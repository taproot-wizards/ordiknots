use anyhow::Result;
use axum::{response::Html, routing::get, Router};
use bitcoin::Txid;
use bitcoin_hashes::Hash;
use bitcoincore_rpc::Client;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::{path::PathBuf, sync::Arc};

use crate::techniques::Technique;

const KNOTWORKS_TABLE: TableDefinition<&[u8; 32], (u8, u64, u64)> =
    TableDefinition::new("knotworks");

#[derive(Clone)]
struct KnotworkInfo {
    txid: String,
    technique: String,
    block_height: u64,
    file_size: u64,
    rendered_content: String, // HTML for displaying the file
}

struct AppState {
    db: Arc<Database>,
    client: Arc<Client>,
}

pub async fn start_server(database_path: PathBuf, port: u16, client: Client) -> Result<()> {
    let db = crate::indexer::open_database(&database_path)?;
    let state = Arc::new(AppState {
        db: Arc::new(db),
        client: Arc::new(client),
    });

    let app = Router::new()
        .route("/", get(list_knotworks))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("Starting server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn list_knotworks(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Result<Html<String>, String> {
    let knotworks = get_all_knotworks(&state.db, &state.client).map_err(|e| e.to_string())?;

    let html = render_html(knotworks);
    Ok(Html(html))
}

fn get_all_knotworks(db: &Database, client: &Client) -> Result<Vec<KnotworkInfo>> {
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(KNOTWORKS_TABLE)?;

    let mut knotworks = Vec::new();

    for result in table.iter()? {
        let (txid_bytes, data) = result?;

        let txid = Txid::from_byte_array(*txid_bytes.value());
        let (technique_u8, block_height, file_size) = data.value();
        let technique = match technique_u8 {
            1 => Technique::ChainedOpReturn,
            2 => Technique::P2wshFakeMultisig,
            _ => continue,
        };

        // Decode the knotwork content
        let rendered_content = match technique.decode(&txid, client) {
            Ok(data) => render_content(&data),
            Err(_) => String::from("<div class=\"error\">Failed to decode</div>"),
        };

        knotworks.push(KnotworkInfo {
            txid: txid.to_string(),
            technique: technique.to_string(),
            block_height,
            file_size,
            rendered_content,
        });
    }

    // Sort by block height, newest first
    knotworks.sort_by(|a, b| b.block_height.cmp(&a.block_height));

    Ok(knotworks)
}

fn render_content(data: &[u8]) -> String {
    // Try to detect content type
    if is_image(data) {
        // Render as base64 image
        let base64 = base64_encode(data);
        let mime_type = detect_image_mime(data);
        format!(
            r#"<img src="data:{};base64,{}" alt="Knotwork image" class="content-img">"#,
            mime_type, base64
        )
    } else if is_text(data) {
        // Render as text preview
        match std::str::from_utf8(data) {
            Ok(text) => {
                let preview = if text.len() > 100 {
                    format!("{}...", &text[..100])
                } else {
                    text.to_string()
                };
                format!(
                    r#"<div class="content-text">{}</div>"#,
                    html_escape(&preview)
                )
            }
            Err(_) => String::from(r#"<div class="content-binary">Binary data</div>"#),
        }
    } else {
        // Unknown content type
        String::from(r#"<div class="content-binary">Binary data</div>"#)
    }
}

fn is_image(data: &[u8]) -> bool {
    // Check for common image magic bytes
    if data.len() < 4 {
        return false;
    }

    // PNG
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return true;
    }
    // JPEG
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return true;
    }
    // GIF
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return true;
    }
    // WebP
    if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        return true;
    }

    false
}

fn is_text(data: &[u8]) -> bool {
    // Simple heuristic: check if most bytes are printable ASCII
    if data.is_empty() {
        return false;
    }

    let printable_count = data
        .iter()
        .filter(|&&b| (0x20..=0x7E).contains(&b) || b == 0x09 || b == 0x0A || b == 0x0D)
        .count();

    (printable_count as f64 / data.len() as f64) > 0.8
}

fn detect_image_mime(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "image/png"
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if data.starts_with(b"GIF") {
        "image/gif"
    } else if data.len() > 12 && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn base64_encode(data: &[u8]) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder =
            base64::write::EncoderWriter::new(&mut buf, &base64::engine::general_purpose::STANDARD);
        encoder.write_all(data).unwrap();
    }
    String::from_utf8(buf).unwrap()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn render_html(knotworks: Vec<KnotworkInfo>) -> String {
    let mut cards = String::new();

    for knotwork in &knotworks {
        let txid_short = if knotwork.txid.len() > 16 {
            format!(
                "{}...{}",
                &knotwork.txid[..8],
                &knotwork.txid[knotwork.txid.len() - 8..]
            )
        } else {
            knotwork.txid.clone()
        };

        let file_size_formatted = format_file_size(knotwork.file_size);

        cards.push_str(&format!(
            r#"
            <div class="card">
                <div class="txid">{}</div>
                <div class="content-preview">
                    {}
                </div>
                <div class="info">
                    <div class="label">Technique:</div>
                    <div class="value">{}</div>
                </div>
                <div class="info">
                    <div class="label">Block:</div>
                    <div class="value">{}</div>
                </div>
                <div class="info">
                    <div class="label">Size:</div>
                    <div class="value">{}</div>
                </div>
            </div>
            "#,
            txid_short,
            knotwork.rendered_content,
            knotwork.technique,
            knotwork.block_height,
            file_size_formatted
        ));
    }

    let count = knotworks.len();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Ordiknots</title>
    <style>
        * {{
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }}

        body {{
            background-color: #1a1a1a;
            color: #e0e0e0;
            font-family: monospace;
            padding: 2rem;
        }}

        h1 {{
            text-align: center;
            margin-bottom: 1rem;
            font-size: 2.5rem;
            color: #ffffff;
        }}

        .subtitle {{
            text-align: center;
            margin-bottom: 3rem;
            color: #888;
            font-size: 1rem;
        }}

        .grid {{
            display: grid;
            grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
            gap: 1.5rem;
            max-width: 1400px;
            margin: 0 auto;
        }}

        .card {{
            background-color: #2a2a2a;
            border: 1px solid #3a3a3a;
            border-radius: 8px;
            padding: 1.5rem;
            transition: border-color 0.3s;
        }}

        .card:hover {{
            border-color: #4a4a4a;
        }}

        .txid {{
            font-size: 1.1rem;
            margin-bottom: 1rem;
            color: #ffffff;
            word-break: break-all;
        }}

        .info {{
            display: flex;
            justify-content: space-between;
            margin-bottom: 0.5rem;
            font-size: 0.9rem;
        }}

        .label {{
            color: #888;
        }}

        .value {{
            color: #e0e0e0;
            font-weight: bold;
        }}

        .content-preview {{
            margin-bottom: 1rem;
            min-height: 150px;
            max-height: 300px;
            overflow: hidden;
            display: flex;
            align-items: center;
            justify-content: center;
            background-color: #1a1a1a;
            border-radius: 4px;
            padding: 0.5rem;
        }}

        .content-img {{
            max-width: 100%;
            max-height: 280px;
            width: auto;
            height: auto;
            object-fit: contain;
        }}

        .content-text {{
            font-size: 0.8rem;
            color: #aaa;
            padding: 0.5rem;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: pre-wrap;
            word-break: break-word;
        }}

        .content-binary {{
            font-size: 0.9rem;
            color: #666;
            font-style: italic;
        }}

        .error {{
            color: #ff6b6b;
            font-size: 0.8rem;
        }}
    </style>
</head>
<body>
    <h1>👿 Ordiknots</h1>
    <div class="subtitle">{} knotwork(s) found</div>
    <div class="grid">
        {}
    </div>
</body>
</html>"#,
        count, cards
    )
}

fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
