use crate::{
    client::{self, git, MessageKind, RequestWriter, SetServiceResponse, WriteMode},
    Service,
};
use bstr::{BString, ByteSlice};
use std::process::{self, Command, Stdio};

// from https://github.com/git/git/blob/20de7e7e4f4e9ae52e6cc7cfaa6469f186ddb0fa/environment.c#L115:L115
const ENV_VARS_TO_REMOVE: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

pub struct SpawnProcessOnDemand {
    path: BString,
    ssh_program: Option<String>,
    ssh_args: Vec<String>,
    ssh_env: Vec<(&'static str, String)>,
    connection: Option<git::Connection<process::ChildStdout, process::ChildStdin>>,
    child: Option<std::process::Child>,
}

impl Drop for SpawnProcessOnDemand {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.wait().ok();
        }
    }
}

impl SpawnProcessOnDemand {
    pub(crate) fn new_ssh(
        program: String,
        args: impl IntoIterator<Item = impl Into<String>>,
        env: impl IntoIterator<Item = (&'static str, impl Into<String>)>,
        path: BString,
    ) -> SpawnProcessOnDemand {
        SpawnProcessOnDemand {
            path: path.into(),
            ssh_program: Some(program),
            ssh_args: args.into_iter().map(|s| s.into()).collect(),
            ssh_env: env.into_iter().map(|(k, v)| (k, v.into())).collect(),
            child: None,
            connection: None,
        }
    }
    pub(crate) fn new(path: BString) -> SpawnProcessOnDemand {
        SpawnProcessOnDemand {
            path: path.into(),
            ssh_program: None,
            ssh_args: Vec::new(),
            ssh_env: Vec::new(),
            child: None,
            connection: None,
        }
    }
}

impl client::Transport for SpawnProcessOnDemand {
    fn handshake(&mut self, service: Service) -> Result<SetServiceResponse, client::Error> {
        assert!(
            self.connection.is_none(),
            "cannot handshake twice with the same connection"
        );
        let mut cmd = match &self.ssh_program {
            Some(program) => Command::new(program),
            None => Command::new(service.as_str()),
        };
        for env_to_remove in ENV_VARS_TO_REMOVE {
            cmd.env_remove(env_to_remove);
        }
        cmd.envs(std::mem::take(&mut self.ssh_env));
        cmd.args(&mut self.ssh_args);
        cmd.stderr(Stdio::null()).stdout(Stdio::piped()).stdin(Stdio::piped());
        if self.ssh_program.is_some() {
            cmd.arg(service.as_str());
        }
        cmd.arg("--strict").arg("--timeout=0").arg(self.path.to_os_str_lossy());

        let mut child = cmd.spawn()?;
        self.connection = Some(git::Connection::new_for_spawned_process(
            child.stdout.take().expect("stdout configured"),
            child.stdin.take().expect("stdin configured"),
            self.path.clone(),
        ));
        self.child = Some(child);
        let c = self
            .connection
            .as_mut()
            .expect("connection to be there right after setting it");
        c.handshake(service)
    }

    fn request(&mut self, write_mode: WriteMode, on_drop: Vec<MessageKind>) -> Result<RequestWriter, client::Error> {
        self.connection
            .as_mut()
            .expect("handshake() to have been called first")
            .request(write_mode, on_drop)
    }
}

pub fn connect(path: impl Into<BString>) -> Result<SpawnProcessOnDemand, std::convert::Infallible> {
    Ok(SpawnProcessOnDemand::new(path.into()))
}
