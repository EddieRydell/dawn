use dawn_lsp::Backend;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = lsp4ij_compatible_stdin(tokio::io::stdin());
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

fn lsp4ij_compatible_stdin(stdin: impl AsyncRead + Unpin + Send + 'static) -> impl AsyncRead {
    let (reader, mut writer) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let _ = forward_lsp_messages(stdin, &mut writer).await;
    });
    reader
}

async fn forward_lsp_messages(
    mut stdin: impl AsyncRead + Unpin,
    writer: &mut (impl AsyncWrite + Unpin),
) -> std::io::Result<()> {
    while let Some(mut body) = read_lsp_message(&mut stdin).await? {
        normalize_shutdown_params(&mut body);
        writer
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await?;
        writer.write_all(&body).await?;
        writer.flush().await?;
    }
    Ok(())
}

async fn read_lsp_message(
    reader: &mut (impl AsyncRead + Unpin),
) -> std::io::Result<Option<Vec<u8>>> {
    let mut header = Vec::new();
    let mut byte = [0; 1];
    while !header.ends_with(b"\r\n\r\n") {
        let read = reader.read(&mut byte).await?;
        if read == 0 {
            return if header.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "incomplete LSP header",
                ))
            };
        }
        header.push(byte[0]);
    }

    let header = String::from_utf8_lossy(&header);
    let content_length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Content-Length")
        })?;

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

fn normalize_shutdown_params(body: &mut Vec<u8>) {
    let Ok(mut message) = serde_json::from_slice::<Value>(body) else {
        return;
    };

    let is_shutdown = message
        .get("method")
        .and_then(Value::as_str)
        .map(|method| method == "shutdown")
        .unwrap_or(false);
    let has_null_params = message.get("params").map(Value::is_null).unwrap_or(false);

    if is_shutdown && has_null_params {
        if let Some(object) = message.as_object_mut() {
            object.remove("params");
        }
        if let Ok(normalized) = serde_json::to_vec(&message) {
            *body = normalized;
        }
    }
}
