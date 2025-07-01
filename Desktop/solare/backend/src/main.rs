/*
 * This is the main entry point for the Rust backend server.
 * It sets up an Actix web server with the token creation logic,
 * including metadata creation and authority management.
 * NOTE: Irys upload functionality is temporarily disabled.
 */

use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder, http::StatusCode};
use serde::{Deserialize, Serialize};
use actix_cors::Cors;
use serde_json::json;
use std::str::FromStr;
// use std::process::Command; // Temporarily unused
// use std::fs::{self, File}; // Temporarily unused
// use std::io::Write; // Temporarily unused
use actix_multipart::Multipart;
use futures_util::stream::StreamExt;


// --- Solana Imports ---
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, read_keypair_file},
    signer::Signer,
    system_instruction,
    system_program,
    transaction::Transaction,
    instruction::Instruction,
    program_pack::Pack,
};
use spl_token::{
    instruction as token_instruction,
    solana_program::native_token::LAMPORTS_PER_SOL,
    state::Mint,
    instruction::AuthorityType,
};
use spl_associated_token_account::get_associated_token_address;

// --- Metaplex Token Metadata Imports ---
use mpl_token_metadata::{
    instructions as metadata_instruction,
    accounts::Metadata,
    types::Creator,
};


// --- Configuration Constants ---
const SOLANA_RPC_URL: &str = "https://api.devnet.solana.com";
const SERVER_WALLET_PATH: &str = "server-wallet.json";
const GEMINI_API_KEY: &str = "AIzaSyCNB-iUE6Ub1k608T4dow2qWFG-EHYjMSk"; // Your Gemini API Key


// --- Data Structures ---

#[derive(Deserialize, Debug)]
struct CreateTokenRequest {
    decimals: u8,
    supply: u64,
    #[serde(rename = "walletAddress")]
    wallet_address: String,
    #[serde(rename = "tokenName")]
    token_name: String,
    #[serde(rename = "tokenSymbol")]
    token_symbol: String,
    description: String,
    #[serde(rename = "revokeMint")]
    revoke_mint: bool,
    #[serde(rename = "revokeFreeze")]
    revoke_freeze: bool,
    #[serde(rename = "revokeUpdate")]
    revoke_update: bool,
}

#[derive(Serialize)]
struct SuccessResponse {
    success: bool,
    message: String,
    token_address: String,
    transaction_id: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    success: bool,
    message: String,
}

impl ErrorResponse {
    fn new(msg: &str) -> Self {
        ErrorResponse {
            success: false,
            message: msg.to_string(),
        }
    }
}

// --- Gemini API Structures ---
#[derive(Deserialize)]
struct IdeaGenerationRequest {
    prompt: String,
}

#[derive(Serialize, Deserialize)]
struct GeminiResponsePart {
    text: String,
}

#[derive(Serialize, Deserialize)]
struct GeminiResponseContent {
    parts: Vec<GeminiResponsePart>,
}

#[derive(Serialize, Deserialize)]
struct GeminiResponseCandidate {
    content: GeminiResponseContent,
}

#[derive(Serialize, Deserialize)]
struct GeminiApiResponse {
    candidates: Vec<GeminiResponseCandidate>,
}


// --- API Endpoints ---

#[get("/health")]
async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(json!({"status": "ok"}))
}

#[post("/generate_ideas")]
async fn generate_ideas(req_body: web::Json<IdeaGenerationRequest>) -> impl Responder {
    let client = reqwest::Client::new();
    let api_url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}", GEMINI_API_KEY);

    let full_prompt = format!(
        "Based on the following concept, generate a creative token name, a 3-5 character stock market-style ticker symbol, and a short, compelling description for a Solana SPL token. The response must be in JSON format. Concept: \"{}\"",
        req_body.prompt
    );

    let payload = json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": full_prompt}]
        }],
        "generationConfig": {
            "responseMimeType": "application/json",
            "responseSchema": {
                "type": "OBJECT",
                "properties": {
                    "name": {"type": "STRING", "description": "The generated name for the token."},
                    "symbol": {"type": "STRING", "description": "A 3-5 character ticker symbol for the token."},
                    "description": {"type": "STRING", "description": "A short, compelling description of the token."}
                },
                "required": ["name", "symbol", "description"]
            }
        }
    });

    match client.post(&api_url).json(&payload).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<GeminiApiResponse>().await {
                    Ok(gemini_response) => {
                        if let Some(candidate) = gemini_response.candidates.get(0) {
                            if let Some(part) = candidate.content.parts.get(0) {
                                // The response from Gemini is a JSON string, so we parse it again
                                match serde_json::from_str::<serde_json::Value>(&part.text) {
                                    Ok(final_json) => HttpResponse::Ok().json(final_json),
                                    Err(_) => HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new("Failed to parse final JSON from Gemini.")),
                                }
                            } else {
                                HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new("No content parts in Gemini response."))
                            }
                        } else {
                             HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new("No candidates in Gemini response."))
                        }
                    }
                    Err(_) => HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new("Failed to parse Gemini API response.")),
                }
            } else {
                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new(&format!("Gemini API request failed: {}", error_text)))
            }
        }
        Err(_) => HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new("Failed to send request to Gemini API.")),
    }
}

#[post("/create_token")]
async fn create_token(mut payload: Multipart) -> impl Responder {
    let mut request_data: Option<CreateTokenRequest> = None;
    // let mut image_path: Option<String> = None; // Temporarily disabled

    // Process the multipart payload
    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(f) => f,
            Err(_) => return HttpResponse::build(StatusCode::BAD_REQUEST).json(ErrorResponse::new("Error processing multipart form.")),
        };

        let content_disposition = field.content_disposition().clone();
        let field_name = content_disposition.get_name().unwrap_or("").to_string();

        if field_name == "data" {
            let mut body = Vec::new();
            while let Some(chunk) = field.next().await {
                body.extend_from_slice(&chunk.unwrap());
            }
            request_data = serde_json::from_slice(&body).ok();
        }
    }

    let req_body = match request_data {
        Some(data) => data,
        None => return HttpResponse::build(StatusCode::BAD_REQUEST).json(ErrorResponse::new("Missing 'data' field in request.")),
    };

    // --- Irys upload steps are temporarily bypassed ---
    // The image upload and metadata JSON creation logic is commented out.
    // We will use an empty string for the metadata URI for now.
    let metadata_uri = String::from("");

    // Proceed with on-chain token creation
    let rpc_client = RpcClient::new(SOLANA_RPC_URL.to_string());

    let server_keypair = match read_keypair_file(SERVER_WALLET_PATH) {
        Ok(kp) => kp,
        Err(_) => return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new("Failed to load server wallet.")),
    };
    
    let user_pubkey = match Pubkey::from_str(&req_body.wallet_address) {
        Ok(pk) => pk,
        Err(_) => return HttpResponse::build(StatusCode::BAD_REQUEST).json(ErrorResponse::new("Invalid user wallet address.")),
    };

    let mint_keypair = Keypair::new();
    
    let rent_lamports = match rpc_client.get_minimum_balance_for_rent_exemption(Mint::LEN) {
        Ok(rent) => rent,
        Err(e) => return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new(&format!("Failed to get rent exemption: {}", e))),
    };

    let token_supply = req_body.supply * 10u64.pow(req_body.decimals as u32);
    let associated_token_address = get_associated_token_address(&user_pubkey, &mint_keypair.pubkey());
    let metadata_pda = Metadata::find_pda(&mint_keypair.pubkey()).0;

    let create_metadata_instruction_args = metadata_instruction::CreateMetadataAccountV3InstructionArgs {
        data: mpl_token_metadata::types::DataV2 {
            name: req_body.token_name.clone(),
            symbol: req_body.token_symbol.clone(),
            uri: metadata_uri, // Use the placeholder metadata URI
            seller_fee_basis_points: 0,
            creators: Some(vec![Creator {
                address: server_keypair.pubkey(),
                verified: true,
                share: 100,
            }]),
            collection: None,
            uses: None,
        },
        is_mutable: !req_body.revoke_update,
        collection_details: None,
    };
    
    let create_metadata_accounts = metadata_instruction::CreateMetadataAccountV3 {
        metadata: metadata_pda,
        mint: mint_keypair.pubkey(),
        mint_authority: server_keypair.pubkey(),
        payer: server_keypair.pubkey(),
        update_authority: (server_keypair.pubkey(), true),
        system_program: system_program::ID,
        rent: None,
    };

    let mut instructions: Vec<Instruction> = vec![
        system_instruction::create_account(
            &server_keypair.pubkey(),
            &mint_keypair.pubkey(),
            rent_lamports,
            Mint::LEN as u64,
            &spl_token::id(),
        ),
        token_instruction::initialize_mint(
            &spl_token::id(),
            &mint_keypair.pubkey(),
            &server_keypair.pubkey(),
            Some(&server_keypair.pubkey()),
            req_body.decimals,
        ).unwrap(),
        spl_associated_token_account::instruction::create_associated_token_account(
            &server_keypair.pubkey(),
            &user_pubkey,
            &mint_keypair.pubkey(),
            &spl_token::id(),
        ),
        token_instruction::mint_to(
            &spl_token::id(),
            &mint_keypair.pubkey(),
            &associated_token_address,
            &server_keypair.pubkey(),
            &[],
            token_supply,
        ).unwrap(),
        create_metadata_accounts.instruction(create_metadata_instruction_args),
    ];

    if req_body.revoke_mint {
        instructions.push(
            token_instruction::set_authority(
                &spl_token::id(),
                &mint_keypair.pubkey(),
                None,
                AuthorityType::MintTokens,
                &server_keypair.pubkey(),
                &[&server_keypair.pubkey()],
            ).unwrap()
        );
    }

    if req_body.revoke_freeze {
        instructions.push(
            token_instruction::set_authority(
                &spl_token::id(),
                &mint_keypair.pubkey(),
                None,
                AuthorityType::FreezeAccount,
                &server_keypair.pubkey(),
                &[&server_keypair.pubkey()],
            ).unwrap()
        );
    }

    let recent_blockhash = match rpc_client.get_latest_blockhash() {
        Ok(bh) => bh,
        Err(e) => return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new(&format!("Failed to get recent blockhash: {}", e))),
    };

    let transaction = Transaction::new_signed_with_payer(
        &instructions, 
        Some(&server_keypair.pubkey()),
        &[&server_keypair, &mint_keypair], 
        recent_blockhash
    );

    match rpc_client.send_and_confirm_transaction_with_spinner(&transaction) {
        Ok(signature) => {
            println!("Transaction successful with signature: {}", signature);
            HttpResponse::Ok().json(SuccessResponse {
                success: true,
                message: "Token created successfully!".to_string(),
                token_address: mint_keypair.pubkey().to_string(),
                transaction_id: signature.to_string(),
            })
        }
        Err(e) => {
            println!("Transaction failed: {}", e);
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(ErrorResponse::new(&format!("Failed to send transaction: {}", e)))
        }
    }
}


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("🚀 Starting SolMint backend server at http://127.0.0.1:8080");

    HttpServer::new(|| {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .service(health_check)
            .service(create_token)
            .service(generate_ideas) // Register the new endpoint
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
