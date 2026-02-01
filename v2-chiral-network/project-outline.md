# Project Requirements
This document outlines v2-chiral-network project only. The goal is to rebuild the original chiral network project but with less prone to errors. You will reference the original chiral network program which is everything outside of the v2-chiral-network folder. 

## User Interface (Frontend)
Important: You should use the old version as a reference since we are trying to rebuild it more efficiently. Even if you reuse code, try to rewrite it to be better and less prone to future issues. 

1. Wallet Creation/Login Page where users have the option to "Create New Wallet" or "Use Existing Wallet". 
    If the user clicks "Create New Wallet" then it should give the user 12 words to act as a "Recovery Phrase". The user can "Copy" or "Regenerate", or "Download as TxT" the phrase using buttons. There is also a cancel button if the user decides not to proceed. After confirming the user has saved the phrases, it will prompt/quiz the user to make sure they actually saved it. The quiz will be to enter 2 of the 12 words (randomly selected). 
    If the user clicks "Use Existing Wallet" then it should allow the user to either use their private key or the 12-word phrase to login.

2. Navbar: Upon logging into the wallet, the main page (and every other page in the application) should have a navbar which contains the following pages: Download, Upload, Account, Network, Settings. There should also be a logout button. There should also be a status to show Disconnect/Connected implemented as a small colored dot (red/green) and the word "Disconnected" or "Connected".

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

You can also reference the old implementation and determine what is useful and what is not and include it into the new implementation and refine it to be better.

Do not create any new MD files. Do not use emojis. Do not use mock/fake data.

