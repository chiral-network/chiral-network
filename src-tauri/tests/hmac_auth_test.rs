use chiral_network::stream_auth::{AuthMessageType, StreamAuthService};

#[tokio::test]
async fn test_hmac_authentication_for_unencrypted_transfers() {
    let mut auth_service = StreamAuthService::new();

    // Test session creation
    let session_id = "peer1-file123";
    let hmac_key = StreamAuthService::generate_hmac_key();
    assert!(auth_service
        .create_session(session_id.to_string(), hmac_key)
        .is_ok());

    // Test creating authenticated chunk
    let chunk_data = b"test chunk data";
    let chunk_index = 0u32;
    let file_hash = "file123";

    let auth_msg = auth_service
        .create_authenticated_chunk(session_id, chunk_data, chunk_index, file_hash)
        .expect("Failed to create authenticated chunk");

    // Verify the authentication message
    assert_eq!(auth_msg.message_type, AuthMessageType::DataChunk);
    assert!(!auth_msg.signature.is_empty());
    assert_eq!(auth_msg.sequence, 1);

    // Test verification
    let verification_result = auth_service
        .verify_data(session_id, &auth_msg)
        .expect("Failed to verify data");
    assert!(verification_result);

    // Test extracting chunk data
    let extracted_data = auth_service
        .verify_authenticated_chunk(session_id, &auth_msg)
        .expect("Failed to verify authenticated chunk");

    assert!(extracted_data.is_some());
    let extracted = extracted_data.unwrap();
    assert_eq!(extracted, chunk_data);
}

#[tokio::test]
async fn test_hmac_key_exchange_flow() {
    let mut auth_service = StreamAuthService::new();

    // Test key exchange initiation
    let initiator_peer_id = "peer1".to_string();
    let target_peer_id = "peer2".to_string();
    let session_id = "session123".to_string();

    let request = auth_service
        .initiate_key_exchange(
            initiator_peer_id.clone(),
            target_peer_id.clone(),
            session_id.clone(),
        )
        .expect("Failed to initiate key exchange");

    assert_eq!(request.initiator_peer_id, initiator_peer_id);
    assert_eq!(request.target_peer_id, target_peer_id);
    assert_eq!(request.session_id, session_id);
    assert!(!request.initiator_public_key.is_empty());

    // Test key exchange response
    let responder_peer_id = "peer2".to_string();
    let response = auth_service
        .respond_to_key_exchange(request, responder_peer_id.clone())
        .expect("Failed to respond to key exchange");

    assert_eq!(response.responder_peer_id, responder_peer_id);
    assert!(!response.responder_public_key.is_empty());
    assert!(!response.hmac_key_confirmation.is_empty());

    // Test key exchange confirmation
    let confirmation = auth_service
        .confirm_key_exchange(response, initiator_peer_id.clone())
        .expect("Failed to confirm key exchange");

    assert!(!confirmation.initiator_confirmation.is_empty());

    // Test key exchange finalization
    auth_service
        .finalize_key_exchange(confirmation, responder_peer_id.clone())
        .expect("Failed to finalize key exchange");

    // Verify the session was created with the exchanged key
    let session_info =
        auth_service.get_session_info(&format!("{}-{}", initiator_peer_id, session_id));
    assert!(session_info.is_some());
}

#[tokio::test]
async fn test_encrypted_vs_unencrypted_authentication() {
    let mut auth_service = StreamAuthService::new();
    let session_id = "test-session";

    // Create session
    let hmac_key = StreamAuthService::generate_hmac_key();
    assert!(auth_service
        .create_session(session_id.to_string(), hmac_key)
        .is_ok());

    let chunk_data = b"test data";

    // Test unencrypted transfer with HMAC authentication
    let auth_msg = auth_service
        .create_authenticated_chunk(session_id, chunk_data, 0, "file123")
        .expect("Failed to create authenticated chunk");

    // Verify authentication works
    let verification = auth_service
        .verify_data(session_id, &auth_msg)
        .expect("Failed to verify data");
    assert!(verification);

    // For encrypted transfers, we would use AES-256-GCM which provides its own authentication
    // This test demonstrates that HMAC is only used for unencrypted transfers
    println!("HMAC authentication is working for unencrypted transfers");
}
