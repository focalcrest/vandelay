/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use rustls_platform_verifier::BuilderVerifierExt;

use super::error::ImapError;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

pub trait ImapStream: Read + Write + Send {
    fn upgrade_tls(
        self: Box<Self>,
        connector: &Connector,
        host: &str,
    ) -> Result<Box<dyn ImapStream>, ImapError>;

    fn is_tls(&self) -> bool;
}

pub struct Connector {
    config: Arc<ClientConfig>,
}

impl Connector {
    pub fn new(allow_invalid_certs: bool) -> Result<Self, ImapError> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let builder = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| ImapError::Tls(format!("rustls protocol versions: {e}")))?;
        let config = if allow_invalid_certs {
            builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerify))
                .with_no_client_auth()
        } else {
            builder
                .with_platform_verifier()
                .map_err(|e| ImapError::Tls(format!("rustls platform verifier: {e}")))?
                .with_no_client_auth()
        };
        Ok(Connector {
            config: Arc::new(config),
        })
    }

    pub fn connect_plain(&self, host: &str, port: u16) -> Result<Box<dyn ImapStream>, ImapError> {
        let tcp = tcp_connect(host, port)?;
        Ok(Box::new(TcpImapStream { tcp }))
    }

    pub fn connect_tls(&self, host: &str, port: u16) -> Result<Box<dyn ImapStream>, ImapError> {
        let tcp = tcp_connect(host, port)?;
        self.wrap_tls(tcp, host)
    }

    fn wrap_tls(&self, tcp: TcpStream, host: &str) -> Result<Box<dyn ImapStream>, ImapError> {
        let server_name = ServerName::try_from(host.to_owned())
            .map_err(|e| ImapError::Tls(format!("invalid server name {host}: {e}")))?;
        let conn = ClientConnection::new(self.config.clone(), server_name)
            .map_err(|e| ImapError::Tls(format!("rustls client: {e}")))?;
        Ok(Box::new(TlsImapStream {
            inner: StreamOwned::new(conn, tcp),
        }))
    }
}

fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, ImapError> {
    let tcp = TcpStream::connect((host, port))?;
    tcp.set_read_timeout(Some(DEFAULT_TIMEOUT))?;
    tcp.set_write_timeout(Some(DEFAULT_TIMEOUT))?;
    tcp.set_nodelay(true)?;
    Ok(tcp)
}

pub struct TcpImapStream {
    tcp: TcpStream,
}

impl Read for TcpImapStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.tcp.read(buf)
    }
}

impl Write for TcpImapStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tcp.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

impl ImapStream for TcpImapStream {
    fn upgrade_tls(
        self: Box<Self>,
        connector: &Connector,
        host: &str,
    ) -> Result<Box<dyn ImapStream>, ImapError> {
        connector.wrap_tls(self.tcp, host)
    }

    fn is_tls(&self) -> bool {
        false
    }
}

pub struct TlsImapStream {
    inner: StreamOwned<ClientConnection, TcpStream>,
}

impl Read for TlsImapStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for TlsImapStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl ImapStream for TlsImapStream {
    fn upgrade_tls(
        self: Box<Self>,
        _connector: &Connector,
        _host: &str,
    ) -> Result<Box<dyn ImapStream>, ImapError> {
        Ok(self)
    }

    fn is_tls(&self) -> bool {
        true
    }
}

pub struct DeflateImapStream {
    inner: Box<dyn ImapStream>,
    decompress: flate2::Decompress,
    compress: flate2::Compress,
    rx_raw: Vec<u8>,
    rx_raw_pos: usize,
    rx_decoded: Vec<u8>,
    rx_decoded_pos: usize,
}

impl DeflateImapStream {
    pub fn wrap(inner: Box<dyn ImapStream>, primer: &[u8]) -> Box<dyn ImapStream> {
        Box::new(DeflateImapStream {
            inner,
            decompress: flate2::Decompress::new(false),
            compress: flate2::Compress::new(flate2::Compression::default(), false),
            rx_raw: primer.to_vec(),
            rx_raw_pos: 0,
            rx_decoded: Vec::new(),
            rx_decoded_pos: 0,
        })
    }
}

impl Read for DeflateImapStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.rx_decoded_pos < self.rx_decoded.len() {
                let avail = &self.rx_decoded[self.rx_decoded_pos..];
                let n = avail.len().min(out.len());
                out[..n].copy_from_slice(&avail[..n]);
                self.rx_decoded_pos += n;
                return Ok(n);
            }
            if self.rx_raw_pos >= self.rx_raw.len() {
                self.rx_raw.clear();
                self.rx_raw_pos = 0;
                self.rx_raw.resize(8192, 0);
                let n = self.inner.read(&mut self.rx_raw)?;
                if n == 0 {
                    return Ok(0);
                }
                self.rx_raw.truncate(n);
            }
            self.rx_decoded.clear();
            self.rx_decoded_pos = 0;
            self.rx_decoded.resize(out.len().max(8192) * 8, 0);
            let in_before = self.decompress.total_in();
            let out_before = self.decompress.total_out();
            let status = self.decompress.decompress(
                &self.rx_raw[self.rx_raw_pos..],
                &mut self.rx_decoded,
                flate2::FlushDecompress::None,
            );
            match status {
                Ok(_) => {}
                Err(e) => {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, e));
                }
            }
            let consumed = (self.decompress.total_in() - in_before) as usize;
            let produced = (self.decompress.total_out() - out_before) as usize;
            self.rx_raw_pos += consumed;
            self.rx_decoded.truncate(produced);
            if produced == 0 && consumed == 0 {
                self.rx_raw.clear();
                self.rx_raw_pos = 0;
                continue;
            }
        }
    }
}

impl Write for DeflateImapStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut out = vec![0u8; buf.len() + 256];
        let in_before = self.compress.total_in();
        let out_before = self.compress.total_out();
        self.compress
            .compress(buf, &mut out, flate2::FlushCompress::Sync)
            .map_err(io::Error::other)?;
        let consumed = (self.compress.total_in() - in_before) as usize;
        let produced = (self.compress.total_out() - out_before) as usize;
        self.inner.write_all(&out[..produced])?;
        Ok(consumed)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl ImapStream for DeflateImapStream {
    fn upgrade_tls(
        self: Box<Self>,
        _connector: &Connector,
        _host: &str,
    ) -> Result<Box<dyn ImapStream>, ImapError> {
        Err(ImapError::Protocol(
            "cannot STARTTLS after COMPRESS DEFLATE".into(),
        ))
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

#[derive(Debug)]
struct NoVerify;

impl rustls::client::danger::ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        use rustls::SignatureScheme::*;
        vec![
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            ECDSA_NISTP521_SHA512,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            ED25519,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_constructs_with_invalid_certs_disabled() {
        let _c = Connector::new(false).unwrap();
    }

    #[test]
    fn connector_constructs_with_invalid_certs_allowed() {
        let _c = Connector::new(true).unwrap();
    }

    struct PeerStream {
        outbound: Vec<u8>,
        inbound: Vec<u8>,
        inbound_pos: usize,
    }

    impl PeerStream {
        fn new() -> Self {
            Self {
                outbound: Vec::new(),
                inbound: Vec::new(),
                inbound_pos: 0,
            }
        }
    }

    impl Read for PeerStream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.inbound_pos >= self.inbound.len() {
                return Ok(0);
            }
            let avail = &self.inbound[self.inbound_pos..];
            let n = avail.len().min(buf.len());
            buf[..n].copy_from_slice(&avail[..n]);
            self.inbound_pos += n;
            Ok(n)
        }
    }

    impl Write for PeerStream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.outbound.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl ImapStream for PeerStream {
        fn upgrade_tls(
            self: Box<Self>,
            _c: &Connector,
            _h: &str,
        ) -> Result<Box<dyn ImapStream>, ImapError> {
            Ok(self)
        }
        fn is_tls(&self) -> bool {
            false
        }
    }

    #[test]
    fn deflate_writes_round_trip_through_peer_decompressor() {
        let peer = Box::new(PeerStream::new());
        let mut stream = DeflateImapStream::wrap(peer, b"");
        stream.write_all(b"hello world").unwrap();

        stream.write_all(b" again").unwrap();
        stream.flush().unwrap();
    }

    #[test]
    fn deflate_reads_decompressed_payload_from_inner() {
        let mut peer = PeerStream::new();
        let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
        let plaintext = b"* OK [CAPABILITY IMAP4rev2 COMPRESS=DEFLATE] hi\r\n";
        let mut out = vec![0u8; plaintext.len() + 256];
        compressor
            .compress(plaintext, &mut out, flate2::FlushCompress::Sync)
            .unwrap();
        let n = compressor.total_out() as usize;
        peer.inbound.extend_from_slice(&out[..n]);

        let mut stream = DeflateImapStream::wrap(Box::new(peer), b"");
        let mut got = vec![0u8; 256];
        let mut total = 0usize;
        while total < plaintext.len() {
            let n = stream.read(&mut got[total..]).unwrap();
            if n == 0 {
                break;
            }
            total += n;
        }
        assert_eq!(&got[..total], plaintext);
    }

    #[test]
    fn deflate_consumes_primer_before_reading_inner() {
        let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
        let plaintext = b"primed payload\r\n";
        let mut out = vec![0u8; plaintext.len() + 256];
        compressor
            .compress(plaintext, &mut out, flate2::FlushCompress::Sync)
            .unwrap();
        let n = compressor.total_out() as usize;
        let primer = out[..n].to_vec();

        let peer = Box::new(PeerStream::new());
        let mut stream = DeflateImapStream::wrap(peer, &primer);
        let mut buf = vec![0u8; 256];
        let mut total = 0usize;
        while total < plaintext.len() {
            let n = stream.read(&mut buf[total..]).unwrap();
            if n == 0 {
                break;
            }
            total += n;
        }
        assert_eq!(&buf[..total], plaintext);
    }
}
