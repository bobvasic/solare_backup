SolMint
SolMint is a decentralized application for creating and launching SPL (Solana Program Library) tokens on the Solana blockchain. It features a React-based frontend and a high-performance Rust backend.

Prerequisites
Before you begin, ensure you have the following installed on your system:

Rust & Cargo: https://www.rust-lang.org/tools/install

Node.js & npm: https://nodejs.org/

Solana Tool Suite: https://docs.solana.com/cli/install

Irys CLI: Run npm install -g @irys/sdk

Python 3: (Usually pre-installed on Linux and macOS)

Project Setup
Follow these steps to set up and run the application locally.

1. Backend Setup
First, navigate to the backend directory and set up the server wallet.

cd backend

Create and Fund Server Wallet:

A local server wallet is required to pay for on-chain transaction fees.

# 1. Create the wallet file
solana-keygen new --outfile server-wallet.json

# 2. Airdrop DevNet SOL to the wallet
solana airdrop 2 --keypair server-wallet.json --url devnet

# 3. (Optional but Recommended) Fund Irys for metadata uploads
# This command uses your server wallet to fund your Irys account for permanent data storage.
irys fund 100000000 -h [https://devnet.irys.xyz](https://devnet.irys.xyz) -w server-wallet.json -t solana -c devnet

2. Running the Servers
You will need to run the backend and frontend servers in two separate terminal windows.

Terminal 1: Start the Backend Server

Navigate to the backend directory and run the application:

# From the project root directory:
cd backend

# Run the Rust server
cargo run

You should see the output: 🚀 Starting SolMint backend server at http://127.0.0.1:8080. Keep this terminal running.

Terminal 2: Start the Frontend Server

Navigate to the project's root directory (the one containing index.html) and start the local web server:

# From the project root directory:
python3 -m http.server

You should see output similar to: Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/).0

3. Accessing the Application
Open your web browser and navigate to:

http://localhost:8000

The SolMint application is now running