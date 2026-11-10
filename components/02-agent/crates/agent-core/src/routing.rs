use crate::{AgentError, Result};
use std::path::{Component, Path, PathBuf};

/// Kind of local assistance the runtime may ask for.
///
/// The model process is never linked into this binary. A route only names
/// where a separate process may be listening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClass {
    Navigate,
    Context,
    Extract,
    Judge,
}

/// Where a local model process is allowed to live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// Absolute Unix socket. The peer is another process.
    UnixSocket(PathBuf),
    /// HTTP on loopback only. Remote hosts are rejected.
    LoopbackHttp { port: u16 },
    /// No model is configured. Callers must proceed without one.
    Absent,
}

/// The model stays outside this process. There is no in-process weight loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustBoundary {
    Outside,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub class: TaskClass,
    pub endpoint: Endpoint,
    pub trust: TrustBoundary,
}

/// Selects a local endpoint. It does not open a socket or send a prompt.
#[derive(Debug, Clone)]
pub struct ModelRouter {
    routes: Vec<Route>,
}

impl ModelRouter {
    /// Every class is unanswered until a person configures a loopback endpoint.
    pub fn local_default() -> Self {
        let routes = [
            TaskClass::Navigate,
            TaskClass::Context,
            TaskClass::Extract,
            TaskClass::Judge,
        ]
        .into_iter()
        .map(|class| Route {
            class,
            endpoint: Endpoint::Absent,
            trust: TrustBoundary::Outside,
        })
        .collect();
        Self { routes }
    }

    pub fn route(&self, class: TaskClass) -> &Route {
        self.routes
            .iter()
            .find(|route| route.class == class)
            .expect("local_default covers every task class")
    }

    pub fn set_endpoint(&mut self, class: TaskClass, endpoint: Endpoint) {
        if let Some(route) = self.routes.iter_mut().find(|route| route.class == class) {
            route.endpoint = endpoint;
            route.trust = TrustBoundary::Outside;
        }
    }

    /// Accept a loopback URL or an absolute `unix://` socket. Reject anything else.
    pub fn parse_endpoint(text: &str) -> Result<Endpoint> {
        let text = text.trim();
        if let Some(path) = text.strip_prefix("unix://") {
            let path = Path::new(path);
            let escapes = path
                .components()
                .any(|component| matches!(component, Component::ParentDir));
            if !path.is_absolute() || escapes {
                return Err(AgentError::Config(
                    "model socket must be an absolute path without '..'".into(),
                ));
            }
            return Ok(Endpoint::UnixSocket(path.to_path_buf()));
        }
        let Some(rest) = text.strip_prefix("http://") else {
            return Err(AgentError::Config(
                "remote model endpoints are outside the trust boundary".into(),
            ));
        };
        if rest.contains('@') || rest.contains('/') {
            return Err(AgentError::Config(
                "model endpoint must be host and port only".into(),
            ));
        }
        let (host, port) = rest
            .rsplit_once(':')
            .ok_or_else(|| AgentError::Config("model endpoint needs a loopback port".into()))?;
        if host != "127.0.0.1" && host != "localhost" {
            return Err(AgentError::Config(
                "model endpoint must stay on loopback".into(),
            ));
        }
        let port: u16 = port.parse().map_err(|_| {
            AgentError::Config("model endpoint port is not a valid TCP port".into())
        })?;
        if port == 0 {
            return Err(AgentError::Config(
                "model endpoint port must be non-zero".into(),
            ));
        }
        Ok(Endpoint::LoopbackHttp { port })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_router_does_not_name_a_model() {
        let router = ModelRouter::local_default();
        let route = router.route(TaskClass::Context);
        assert_eq!(route.endpoint, Endpoint::Absent);
        assert_eq!(route.trust, TrustBoundary::Outside);
    }

    #[test]
    fn remote_and_wildcard_endpoints_are_rejected() {
        assert!(ModelRouter::parse_endpoint("https://api.openai.com/v1").is_err());
        assert!(ModelRouter::parse_endpoint("http://0.0.0.0:11434").is_err());
        assert!(ModelRouter::parse_endpoint("http://10.0.0.8:11434").is_err());
        assert!(ModelRouter::parse_endpoint("unix://../sock").is_err());
        let local = ModelRouter::parse_endpoint("http://127.0.0.1:11434").unwrap();
        assert_eq!(local, Endpoint::LoopbackHttp { port: 11434 });
        let socket = ModelRouter::parse_endpoint("unix:///run/user/1000/model.sock").unwrap();
        assert_eq!(
            socket,
            Endpoint::UnixSocket(PathBuf::from("/run/user/1000/model.sock"))
        );
    }
}
