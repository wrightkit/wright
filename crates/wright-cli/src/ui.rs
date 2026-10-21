use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use wright_driver::config::SourceKind;
#[cfg(not(feature = "ui"))]
use wright_driver::result::exit;

use crate::serve::{LintPolicyArgs, parse_kind, parse_profile};

/// `wright ui` takes the same input default and session options as
/// `wright serve` — frontend, locale, profile, lint policy, and an INPUT
/// path defaulting to the current directory — plus `--no-open` for
/// headless use.
#[derive(Debug, Args)]
pub(crate) struct UiArgs {
    /// Input frontend.
    #[arg(long, value_parser = parse_kind, default_value = "auto")]
    kind: SourceKind,
    /// Workshop client locale override.
    #[arg(long, value_name = "LOCALE")]
    locale: Option<String>,
    /// WIR transformation policy.
    #[arg(long, value_parser = parse_profile, default_value = "off")]
    profile: wright_driver::Profile,
    #[command(flatten)]
    policy: LintPolicyArgs,
    /// Print the session URL instead of opening the default browser.
    #[arg(long)]
    no_open: bool,
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,
}

pub(crate) fn run(args: UiArgs) -> ExitCode {
    #[cfg(feature = "ui")]
    {
        server::run(args)
    }
    #[cfg(not(feature = "ui"))]
    {
        let _ = args;
        eprintln!(
            "wright: this build does not include `wright ui`; the browser view is embedded only in builds with the 'ui' feature"
        );
        ExitCode::from(exit::USAGE)
    }
}

#[cfg(feature = "ui")]
mod server {
    use super::UiArgs;
    use std::io::{Cursor, Read};
    use std::net::{IpAddr, Ipv4Addr};
    use std::path::Path;
    use std::process::ExitCode;

    use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
    use wright_driver::edit::EditTransaction;
    use wright_driver::result::exit;
    use wright_driver::service::ToolService;
    use wright_driver::source_provider::SourceBackend;

    use crate::serve;

    include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

    const API_PATH: &str = "/api";
    const APPLY_PATH: &str = "/apply";
    const MAX_BODY: u64 = 1024 * 1024;

    impl UiArgs {
        fn session_config(&self) -> Result<SessionConfig, String> {
            if self
                .input
                .as_deref()
                .is_some_and(|input| input == Path::new("-"))
            {
                return Err(
                    "pass a source file or project directory; `ui` does not read the program from stdin"
                        .to_string(),
                );
            }
            let mut config = SessionConfig {
                kind: self.kind,
                source_backend: SourceBackend::Auto,
                locale: self.locale.clone(),
                profile: self.profile,
                input: InputSpec::Path(self.input.clone().unwrap_or_else(|| ".".into())),
                ..SessionConfig::default()
            };
            if self.profile != wright_driver::Profile::Off
                && matches!(self.kind, SourceKind::Opy | SourceKind::Ostw)
            {
                return Err(
                    "--profile applies to Workshop input only; remove it or pass --kind workshop"
                        .to_string(),
                );
            }
            self.policy.apply_to(&mut config)?;
            Ok(config)
        }
    }

    pub(super) fn run(args: UiArgs) -> ExitCode {
        let config = match args.session_config() {
            Ok(config) => config,
            Err(message) => {
                eprintln!("wright: {message}");
                return ExitCode::from(exit::USAGE);
            }
        };
        let resolved = match wright_driver::input::resolve(&config) {
            Ok(resolved) => resolved,
            Err(diagnostic) => {
                eprintln!("wright: {}", diagnostic.message);
                return ExitCode::from(exit::SOURCE_ERROR);
            }
        };
        match resolved.kind {
            SourceKind::Workshop => {}
            SourceKind::Opy => {
                eprintln!(
                    "wright: `ui` views raw Workshop projects; this input is OPY, which opy-rs implements"
                );
                return ExitCode::from(exit::UNSUPPORTED);
            }
            SourceKind::Ostw => {
                eprintln!(
                    "wright: `ui` views raw Workshop projects; this input is OSTW, which deltin-rs implements"
                );
                return ExitCode::from(exit::UNSUPPORTED);
            }
            other => {
                eprintln!("wright: `ui` is not defined for '{}' input", other.as_str());
                return ExitCode::from(exit::UNSUPPORTED);
            }
        }

        let mut session = match wright_driver::CompilerSession::new(config) {
            Ok(session) => session,
            Err(diagnostic) => {
                eprintln!("wright: {}", diagnostic.message);
                return ExitCode::from(exit::USAGE);
            }
        };
        let mut service = match ToolService::new(&mut session) {
            Ok(service) => service,
            Err(diagnostic) => {
                eprintln!("wright: {}", diagnostic.message);
                return ExitCode::from(exit::USAGE);
            }
        };

        let server = match tiny_http::Server::http((Ipv4Addr::LOCALHOST, 0)) {
            Ok(server) => server,
            Err(error) => {
                eprintln!("wright: cannot bind the UI server on loopback: {error}");
                return ExitCode::from(exit::INTERNAL);
            }
        };
        let Some(addr) = server.server_addr().to_ip() else {
            eprintln!("wright: the UI server did not bind a TCP loopback address");
            return ExitCode::from(exit::INTERNAL);
        };
        debug_assert!(addr.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST));

        let token = match session_token() {
            Some(token) => token,
            None => {
                eprintln!("wright: cannot initialize the per-session token");
                return ExitCode::from(exit::INTERNAL);
            }
        };
        // The token rides the URL fragment, which browsers never send back;
        // endpoint requests carry it in `Authorization` instead.
        let url = format!("http://{addr}/#{token}");
        println!("{url}");
        eprintln!("wright ui: serving the project view; press Ctrl-C to stop");
        if !args.no_open {
            open_browser(&url);
        }

        let authority = addr.to_string();
        let expected_origin = format!("http://{authority}");
        for mut request in server.incoming_requests() {
            let response = route(
                &mut service,
                &mut request,
                &authority,
                &expected_origin,
                &token,
            );
            let _ = request.respond(response);
        }
        ExitCode::SUCCESS
    }

    fn session_token() -> Option<String> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).ok()?;
        let mut token = String::with_capacity(32);
        for byte in bytes {
            token.push_str(&format!("{byte:02x}"));
        }
        Some(token)
    }

    fn route(
        service: &mut ToolService<'_>,
        request: &mut tiny_http::Request,
        authority: &str,
        origin: &str,
        token: &str,
    ) -> tiny_http::Response<Cursor<Vec<u8>>> {
        if header(request, "Host") != Some(authority) {
            return respond(
                403,
                "text/plain; charset=utf-8",
                "forbidden: this UI is bound to its own loopback address"
                    .as_bytes()
                    .to_vec(),
            );
        }
        if let Some(sent) = header(request, "Origin")
            && sent != origin
        {
            return respond(
                403,
                "text/plain; charset=utf-8",
                "forbidden: the request origin is not this UI"
                    .as_bytes()
                    .to_vec(),
            );
        }
        let path = request.url().split(['?', '#']).next().unwrap_or("/");
        let path = path
            .strip_prefix("http://")
            .or_else(|| path.strip_prefix("https://"))
            .and_then(|rest| rest.find('/').map(|index| &rest[index..]))
            .unwrap_or(path);
        match (request.method().as_str(), path) {
            ("POST", API_PATH) => api(service, request, token),
            ("POST", APPLY_PATH) => apply(service, request, token),
            ("GET", "/") => asset("index.html"),
            ("GET", name) => asset(name.trim_start_matches('/')),
            _ => respond(404, "text/plain; charset=utf-8", b"not found".to_vec()),
        }
    }

    fn read_request_body(
        request: &mut tiny_http::Request,
        token: &str,
    ) -> Result<String, tiny_http::Response<Cursor<Vec<u8>>>> {
        let expected = format!("Bearer {token}");
        if header(request, "Authorization") != Some(expected.as_str()) {
            return Err(respond(
                403,
                "text/plain; charset=utf-8",
                b"forbidden: missing or wrong session token".to_vec(),
            ));
        }
        let mut body = Vec::new();
        let read = request
            .as_reader()
            .take(MAX_BODY + 1)
            .read_to_end(&mut body);
        if read.is_err() || body.len() as u64 > MAX_BODY {
            return Err(respond(
                400,
                "application/json",
                br#"{"error":{"code":"malformed-request","message":"request body is unreadable or too large"}}"#
                    .to_vec(),
            ));
        }
        String::from_utf8(body).map_err(|_| {
            respond(
                400,
                "application/json",
                br#"{"error":{"code":"malformed-request","message":"request is not valid UTF-8"}}"#
                    .to_vec(),
            )
        })
    }

    fn api(
        service: &mut ToolService<'_>,
        request: &mut tiny_http::Request,
        token: &str,
    ) -> tiny_http::Response<Cursor<Vec<u8>>> {
        let body = match read_request_body(request, token) {
            Ok(body) => body,
            Err(response) => return response,
        };
        respond(
            200,
            "application/json",
            format!("{}\n", serve::dispatch(service, &body)).into_bytes(),
        )
    }

    fn apply(
        service: &mut ToolService<'_>,
        request: &mut tiny_http::Request,
        token: &str,
    ) -> tiny_http::Response<Cursor<Vec<u8>>> {
        let body = match read_request_body(request, token) {
            Ok(body) => body,
            Err(response) => return response,
        };
        let transaction = match serde_json::from_str::<EditTransaction>(&body) {
            Ok(transaction) => transaction,
            Err(_) => {
                return respond(
                    400,
                    "application/json",
                    br#"{"error":{"code":"malformed-request","message":"the apply body is not an edit transaction"}}"#
                        .to_vec(),
                );
            }
        };
        let result = service.apply_edit_transaction(&transaction);
        respond(
            200,
            "application/json",
            format!("{}\n", serde_json::to_string(&result).expect("serializes")).into_bytes(),
        )
    }

    fn asset(name: &str) -> tiny_http::Response<Cursor<Vec<u8>>> {
        let found = if name.split('/').all(|part| part != "..") && !name.contains('\\') {
            UI_ASSETS.iter().find(|(path, _, _)| *path == name)
        } else {
            None
        };
        match found {
            Some((_, content_type, bytes)) => respond(200, content_type, bytes.to_vec()),
            None => respond(404, "text/plain; charset=utf-8", b"not found".to_vec()),
        }
    }

    fn header<'a>(request: &'a tiny_http::Request, name: &'static str) -> Option<&'a str> {
        request
            .headers()
            .iter()
            .find(|header| header.field.equiv(name))
            .map(|header| header.value.as_str())
    }

    fn respond(
        status: u16,
        content_type: &str,
        body: Vec<u8>,
    ) -> tiny_http::Response<Cursor<Vec<u8>>> {
        tiny_http::Response::from_data(body)
            .with_status_code(status)
            .with_header(
                tiny_http::Header::from_bytes("Content-Type".as_bytes(), content_type.as_bytes())
                    .expect("content types are ASCII"),
            )
    }

    fn open_browser(url: &str) {
        #[cfg(target_os = "macos")]
        let spawned = std::process::Command::new("open").arg(url).spawn();
        #[cfg(target_os = "windows")]
        let spawned = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let spawned = std::process::Command::new("xdg-open").arg(url).spawn();
        #[cfg(not(any(unix, windows)))]
        let spawned: Result<std::process::Child, std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "no known system browser opener",
        ));
        if let Err(error) = spawned {
            eprintln!("wright: cannot open the default browser ({error}); open {url} manually");
        }
    }
}
