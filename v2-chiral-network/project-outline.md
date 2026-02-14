# Project Requirements
This document outlines v2-chiral-network project only. The goal is to rebuild the original chiral network project but with less prone to errors. You will reference the original chiral network program which is everything outside of the v2-chiral-network folder. 

## User Interface (Frontend)
Important: You should use the old version as a reference since we are trying to rebuild it more efficiently. Even if you reuse code, try to rewrite it to be better and less prone to future issues. 

1. Wallet Creation/Login Page where users have the option to "Create New Wallet" or "Use Existing Wallet". 
    If the user clicks "Create New Wallet" then it should give the user 12 words to act as a "Recovery Phrase". The user can "Copy" or "Regenerate", or "Download as TxT" the phrase using buttons. There is also a cancel button if the user decides not to proceed. After confirming the user has saved the phrases, it will prompt/quiz the user to make sure they actually saved it. The quiz will be to enter 2 of the 12 words (randomly selected). 
    If the user clicks "Use Existing Wallet" then it should allow the user to either use their private key or the 12-word phrase to login.

2. Navbar: Upon logging into the wallet, the main page (and every other page in the application) should have a navbar which contains the following pages: Download, Upload, Account, Network, Settings. There should also be a logout button. There should also be a status to show Disconnect/Connected implemented as a small colored dot (red/green) and the word "Disconnected" or "Connected".

3. 

## Frontend + Backend Implementation
Important: You should use the old version as a reference since we are trying to rebuild it more efficiently. Fix any bad code and remove any useless code. Also, update any frontend as needed.

We can reuse the Bootstrap and anything that was made/hosted outside the program. Rewrite and modify the code for it in the new project iteration.

1. Network + Network Tab: Using some of the old implementation's code, we want users to be able to connect to the network and for everyone to be able to see each other on the network. Some things to include:
    - number of connected users
    - ability to connect/disconnect from DHT network and any information that could be relevant such as peer id, port, multiaddress, etc
    - map of geographic distribution of users
    - Other data/statistics that are useful
    - the ability to see other users on the network.
If I connect from a second device, I should be able to see other clients.

2. ChiralDrop: Basically airdrop. Assign users an alias (changes everytime; Color + Animal combination) that allows others to identify the user. The first layer of the page should have a map where user icons pop up on a wave. Users will be able to click the icon of another user and transfer files. Upon receipt, a user can choose to accept or decline the file. We will also track the transaction history (uploads and downloads). Make sure the transaction history persists. 

3. Upload Page: We want to support the following two protocols: WebRTC and BitTorrent using libp2p. We will not be bringing FTP or ed2k from the old implementation as they either are outdated, flawed, or do not make sense in this application. Allow users to choose between the two protocols and allow users to upload a file via a drag and drop or through a button that allows them to access the file explorer. Upon uploading, there should also be a Upload History which includes the file name, Merkle Hash, Size, file type, protocol used, and any other relevant information. Users should also be able to remove/unupload a file. 

4. Download Page: We want to let users download the files that were uploaded. Allow the user to add new download by either searching via Merkle Root Hash (sha-256) or .torrent file or magent link. There should also be a download tracker which shows the states: active, paused, completed, cancelled, queued, or failed. Anything finished should also be shown in a download history section. 

5. Account Page: Create an area for Wallet information such as balance (in Chiral), Chiral Address (should be hideable, copyable), private key (should be hideable, copyable). In addition, include transaction history. There should also be the ability to send chiral coins from one user to another. To send, a client can use the recipient address. Include a confirmation button for sending chirals. For testing purposes, give a default balance of 1 chiral to everyone who creates a wallet.

Ensure all steps are implemented and working properly.

You can also reference the old implementation and determine what is useful and what is not and include it into the new implementation and refine it to be better.

Do not create any new MD files. Do not use emojis. Do not use mock/fake data. No placeholders. You must implement every feature in full.

