use anyhow::{Result, Context, ensure};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

const TLS_START_BYTE: u8 = 0x16;

pub async fn detect<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Proto> {
    let buf = reader.fill_buf().await
        .context("Failed to fill buffer")?;

    ensure!(!buf.is_empty(), "End of stream");

    let first_byte = buf[0];

    Ok(match first_byte {
        TLS_START_BYTE => Proto::Tls,
        _ => Proto::Plain,
    })
}

#[derive(PartialEq, Eq, Debug)]
pub enum Proto {
    Plain,
    Tls,
}
