
# Proxy Self-Test Guide

This document provides instructions on how to test the proxy functionality of the Chiral Network application.

## Running the Test

1.  **Start the application in development mode:**

    ```bash
    npm run tauri:dev
    ```

2.  **Open the Proxy Self-Test Page:**

    Once the application is running, navigate to the `/proxy-self-test` page in your browser.

3.  **Connect to a Proxy:**

    -   Enter the URL of the proxy server (e.g., `ws://127.0.0.1:4001`).
    -   Enter the authentication token for the proxy.
    -   Click the "Connect" button.

4.  **Verify Connection Status:**

    -   You should see the proxy appear in the list with a "connecting" status.
    -   After a few seconds, the status should change to "online".
    -   Check the browser's developer console for `proxy_status_update` log messages.

5.  **Disconnect from a Proxy:**

    -   Click the "Disconnect" button next to the proxy you want to disconnect from.
    -   The status of the proxy should change to "offline".

6.  **Check Rust Logs:**

    You can view the Rust logs by running the application with `RUST_LOG=info`:

    ```bash
    RUST_LOG=info npm run tauri:dev
    ```
