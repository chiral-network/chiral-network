// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/**
 * @title ProofOfStorage
 * @dev This contract manages the registration of files via their Merkle roots
 * and issues periodic challenges to storage providers to ensure file availability.
 * This corresponds to Phase 2 of the Proof of Storage system.
 */
contract ProofOfStorage {
    // --- Structs ---

    struct FileInfo {
        address owner;
        uint256 totalChunks;
        uint256 registrationBlock;
        bool exists;
    }

    // --- State Variables ---

    // Mapping from a file's Merkle root to its metadata.
    mapping(bytes32 => FileInfo) public files;

    // An array of all registered file Merkle roots to enable iteration for challenges.
    bytes32[] public fileRoots;

    // Mapping to track the block number of the last challenge for each file.
    mapping(bytes32 => uint256) public lastChallengeBlock;

    // The frequency (in blocks) at which challenges can be issued for a file.
    uint256 public challengeInterval;

    // --- Events ---

    /**
     * @dev Emitted when a new file is registered on the contract.
     * @param fileRoot The Merkle root of the file, serving as its unique ID.
     * @param owner The address of the file owner.
     * @param totalChunks The total number of chunks the file is composed of.
     */
    event FileRegistered(
        bytes32 indexed fileRoot,
        address indexed owner,
        uint256 totalChunks
    );

    /**
     * @dev Emitted when a storage challenge is generated for a specific file.
     * @param fileRoot The Merkle root of the challenged file.
     * @param chunkIndex The specific chunk index that must be proven.
     */
    event ChallengeIssued(bytes32 indexed fileRoot, uint256 chunkIndex);

    /**
     * @dev Emitted when a proof is successfully verified.
     * @param fileRoot The Merkle root of the file for which the proof was verified.
     * @param provider The address that submitted the valid proof.
     */
    event ProofVerified(bytes32 indexed fileRoot, address indexed provider);

    // --- Constructor ---

    /**
     * @dev Sets the initial challenge interval when the contract is deployed.
     * @param _challengeInterval The number of blocks that must pass before a new
     *        challenge can be issued for the same file. A value of 240 is ~1 hour on a 15s block time chain.
     */
    constructor(uint256 _challengeInterval) {
        challengeInterval = _challengeInterval;
    }

    // --- Functions ---

    /**
     * @dev Registers a new file's Merkle root, making it eligible for challenges.
     * This function would typically be called by the uploader (or a service acting on their behalf).
     * @param _fileRoot The Merkle root of the file.
     * @param _totalChunks The total number of chunks in the file.
     */
    function registerFile(bytes32 _fileRoot, uint256 _totalChunks) external {
        require(!files[_fileRoot].exists, "File is already registered");
        require(_totalChunks > 0, "File must have at least one chunk");

        files[_fileRoot] = FileInfo({
            owner: msg.sender,
            totalChunks: _totalChunks,
            registrationBlock: block.number,
            exists: true
        });

        fileRoots.push(_fileRoot);

        emit FileRegistered(_fileRoot, msg.sender, _totalChunks);
    }

    /**
     * @dev Generates and issues a challenge for a given file.
     * This can be called by anyone, creating a decentralized way to trigger proofs.
     * @param _fileRoot The Merkle root of the file to challenge.
     */
    function issueChallenge(bytes32 _fileRoot) external {
        FileInfo storage file = files[_fileRoot];
        require(file.exists, "File not registered");
        require(
            block.number >= lastChallengeBlock[_fileRoot] + challengeInterval,
            "Challenge interval not met"
        );

        // Generate a pseudo-random chunk index to challenge.
        // Using a simple hash-based approach for on-chain randomness.
        // Note: This is susceptible to miner manipulation but is sufficient for many use cases.
        uint256 randomChunkIndex = uint256(keccak256(abi.encodePacked(block.timestamp, block.prevrandao, _fileRoot))) % file.totalChunks;

        lastChallengeBlock[_fileRoot] = block.number;

        emit ChallengeIssued(_fileRoot, randomChunkIndex);
    }

    /**
     * @dev Verifies a Merkle proof for a given file chunk.
     * This function is called by a storage provider in response to a challenge.
     * @param _fileRoot The Merkle root of the file being proven.
     * @param _proof A dynamic array of sibling hashes in the Merkle tree.
     * @param _chunkData The raw data of the challenged chunk.
     * @param _chunkIndex The index of the challenged chunk.
     * @return A boolean indicating if the proof is valid.
     */
    function verifyProof(
        bytes32 _fileRoot,
        bytes32[] calldata _proof,
        bytes calldata _chunkData,
        uint256 _chunkIndex
    ) external view returns (bool) {
        require(files[_fileRoot].exists, "File not registered");

        // 1. Hash the provided chunk data to get the leaf hash.
        bytes32 leafHash = keccak256(_chunkData);

        // 2. Iteratively compute the Merkle root from the leaf and the proof.
        bytes32 computedRoot = leafHash;
        uint256 proofIndex = _chunkIndex;

        for (uint256 i = 0; i < _proof.length; i++) {
            bytes32 sibling = _proof[i];
            if (proofIndex % 2 == 0) {
                // If the current index is even, hash it with the sibling on the right.
                computedRoot = keccak256(abi.encodePacked(computedRoot, sibling));
            } else {
                // If the current index is odd, hash it with the sibling on the left.
                computedRoot = keccak256(abi.encodePacked(sibling, computedRoot));
            }
            // Move up to the next level of the tree.
            proofIndex /= 2;
        }

        // 3. Compare the computed root with the registered file root.
        return computedRoot == _fileRoot;
    }
}