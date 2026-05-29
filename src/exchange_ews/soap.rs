/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use crate::exchange_ews::types::ServerVersion;

pub const NS_SOAP: &str = "http://schemas.xmlsoap.org/soap/envelope/";
pub const NS_TYPES: &str = "http://schemas.microsoft.com/exchange/services/2006/types";
pub const NS_MESSAGES: &str = "http://schemas.microsoft.com/exchange/services/2006/messages";

pub const SOAP_ACTION_BASE: &str = "http://schemas.microsoft.com/exchange/services/2006/messages";

#[derive(Debug, Clone, Copy)]
pub struct EnvelopeOptions<'a> {
    pub version: ServerVersion,
    pub impersonated_smtp: Option<&'a str>,
}

impl Default for EnvelopeOptions<'_> {
    fn default() -> Self {
        EnvelopeOptions {
            version: ServerVersion::Exchange2013Sp1,
            impersonated_smtp: None,
        }
    }
}

pub fn wrap_envelope(opts: EnvelopeOptions<'_>, body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 512);
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>");
    out.push_str("<soap:Envelope xmlns:soap=\"");
    out.push_str(NS_SOAP);
    out.push_str("\" xmlns:t=\"");
    out.push_str(NS_TYPES);
    out.push_str("\" xmlns:m=\"");
    out.push_str(NS_MESSAGES);
    out.push_str("\"><soap:Header>");
    out.push_str("<t:RequestServerVersion Version=\"");
    out.push_str(opts.version.as_str());
    out.push_str("\"/>");
    if let Some(smtp) = opts.impersonated_smtp {
        out.push_str("<t:ExchangeImpersonation><t:ConnectingSID><t:PrimarySmtpAddress>");
        write_escaped(&mut out, smtp);
        out.push_str("</t:PrimarySmtpAddress></t:ConnectingSID></t:ExchangeImpersonation>");
    }
    out.push_str("</soap:Header><soap:Body>");
    out.push_str(body);
    out.push_str("</soap:Body></soap:Envelope>");
    out
}

pub fn soap_action(operation: &str) -> String {
    format!("\"{SOAP_ACTION_BASE}/{operation}\"")
}

pub fn write_escaped(out: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_has_canonical_namespaces() {
        let env = wrap_envelope(EnvelopeOptions::default(), "<m:FindFolder/>");
        assert!(env.contains("xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\""));
        assert!(
            env.contains("xmlns:t=\"http://schemas.microsoft.com/exchange/services/2006/types\"")
        );
        assert!(
            env.contains(
                "xmlns:m=\"http://schemas.microsoft.com/exchange/services/2006/messages\""
            )
        );
        assert!(env.contains("<t:RequestServerVersion Version=\"Exchange2013_SP1\"/>"));
        assert!(env.contains("<m:FindFolder/>"));
    }

    #[test]
    fn envelope_impersonation_includes_smtp() {
        let opts = EnvelopeOptions {
            version: ServerVersion::Exchange2016,
            impersonated_smtp: Some("alice@contoso.com"),
        };
        let env = wrap_envelope(opts, "<m:FindFolder/>");
        assert!(env.contains("<t:ExchangeImpersonation>"));
        assert!(env.contains("<t:PrimarySmtpAddress>alice@contoso.com</t:PrimarySmtpAddress>"));
        assert!(env.contains("Version=\"Exchange2016\""));
    }

    #[test]
    fn soap_action_quotes_operation() {
        assert_eq!(
            soap_action("FindItem"),
            "\"http://schemas.microsoft.com/exchange/services/2006/messages/FindItem\""
        );
    }

    #[test]
    fn write_escaped_handles_metachars() {
        let mut s = String::new();
        write_escaped(&mut s, "<a&b>");
        assert_eq!(s, "&lt;a&amp;b&gt;");
    }
}
