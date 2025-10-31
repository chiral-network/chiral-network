import { ethers } from "hardhat";

async function main() {
  // The challenge interval in blocks.
  // As noted in the contract, a value of 240 is ~1 hour on a 15s block time chain.
  const challengeInterval = 240;

  console.log("Deploying ProofOfStorage contract...");

  const proofOfStorage = await ethers.deployContract("ProofOfStorage", [
    challengeInterval,
  ]);

  await proofOfStorage.waitForDeployment();

  const contractAddress = await proofOfStorage.getAddress();
  console.log(`ProofOfStorage deployed to: ${contractAddress}`);
}

// We recommend this pattern to be able to use async/await everywhere
// and properly handle errors.
main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
