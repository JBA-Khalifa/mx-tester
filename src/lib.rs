use std::{
    borrow::Cow,
    collections::HashMap,
    ffi::{OsStr, OsString},
    io::{Error, ErrorKind},
    path::PathBuf,
    str::FromStr,
};

use itertools::Itertools;
use lazy_static::lazy_static;
use serde::Deserialize;

lazy_static! {
    /// Environment variable: the directory where a given module should be copied.
    ///
    /// Passed to `build` scripts.
    static ref MX_TEST_MODULE_DIR: OsString = OsString::from_str("MX_TEST_MODULE_DIR").unwrap();


    /// The docker tag used for the Synapse image we produce.
    static ref PATCHED_IMAGE_DOCKER_TAG: OsString = OsString::from_str("mx-tester/synapse").unwrap();

    /// An empty environment.
    static ref EMPTY_ENV: HashMap<&'static OsStr, Cow<'static, OsStr>> = HashMap::new();
}

/// The result of the test, as seen by `down()`.
pub enum Status {
    /// The test was a success.
    Success,

    /// The test was a failure.
    Failure,

    /// The test was not executed at all, we just ran `mx-tester down`.
    Manual,
}

pub enum SynapseVersion {
    /// The latest version of Synapse released on https://hub.docker.com/r/matrixdotorg/synapse/
    ReleasedDockerImage,
    // FIXME: Allow using a version of Synapse that lives in a local directory
    // (this will be sufficient to also implement pulling from github develop)
}
impl SynapseVersion {
    pub fn tag(&self) -> Cow<'static, OsStr> {
        let tag: &'static OsStr = PATCHED_IMAGE_DOCKER_TAG.as_ref();
        tag.into()
    }
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct Script {
    /// The lines of the script.
    ///
    /// Passed without change to `std::process::Command`.
    ///
    /// To communicate with the script, clients should use
    /// an exchange file.
    lines: Vec<String>,
}
impl Script {
    pub fn run(&self, env: &HashMap<&'static OsStr, Cow<'_, OsStr>>) -> Result<(), Error> {
        for line in &self.lines {
            let status = std::process::Command::new(&line)
                .envs(env)
                .spawn()?
                .wait()?;
            if !status.success() {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "Error running command `{line}`: {status}",
                        line = line,
                        status = status
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// A script for `build`.
#[derive(Debug, Deserialize)]
pub struct ModuleConfig {
    /// The name of the module.
    ///
    /// This name is used to create a subdirectory.
    name: String,

    /// A script to build and copy the module in the directory
    /// specified by environment variable `MX_TEST_MODULE_DIR`.
    build: Script,
}

/// A script for `down`.
#[derive(Debug, Deserialize)]
pub struct DownScript {
    /// Code to run in case the test is a success.
    success: Option<Script>,

    /// Code to run in case the test is a failure.
    failure: Option<Script>,

    /// Code to run regardless of the result of the test.
    ///
    /// Executed after `success` or `failure`.
    finally: Option<Script>,
}

fn synapse_root() -> PathBuf {
    std::env::temp_dir().join("mx-tester").join("synapse")
}

/// Rebuild the Synapse image with modules.
pub fn build(
    config: &[ModuleConfig],
    version: SynapseVersion,
    modules: &[&str],
) -> Result<(), Error> {
    let synapse_root = synapse_root();
    std::fs::create_dir_all(&synapse_root)
        .unwrap_or_else(|err| panic!("Cannot create directory {:?}: {}", synapse_root, err));

    // Build modules
    for module in config {
        let mut env: HashMap<&'static OsStr, _> = HashMap::with_capacity(1);
        let path = synapse_root.join(&module.name);
        env.insert(&*MX_TEST_MODULE_DIR, path.as_os_str().into());
        module.build.run(&env)?;
    }

    // Prepare Dockerfile including modules.
    let dockerfile_content = format!("
        # A custom Dockerfile to rebuild synapse from the official release + plugins

        FROM matrixdotorg/synapse:latest
        
        # We need gcc to build pyahocorasick
        RUN apt-get update --quiet && apt-get install gcc --yes --quiet
        
        # Show the Synapse version, to aid with debugging.
        RUN pip show matrix-synapse

        # Copy and install custom modules.
        RUN mkdir /mx-tester
        {copy}
        
        VOLUME [\"/data\"]
        
        EXPOSE 8008/tcp 8009/tcp 8448/tcp
",
    copy = modules.iter()
        .map(|module| format!("COPY {module}, /mx-tester/{module}\n RUN /usr/local/bin/python -m pip install /mx-tester/{module}", module=module))
        .format("\n")
);

    let docker_dir_path = std::env::temp_dir().join("mx-tester").join("docker");
    std::fs::create_dir_all(&docker_dir_path).unwrap_or_else(|err| {
        panic!(
            "Could not create directory `{:?}`: {}",
            &docker_dir_path, err
        )
    });
    let dockerfile_path = docker_dir_path.join("Dockerfile");
    std::fs::write(&dockerfile_path, dockerfile_content)
        .unwrap_or_else(|err| panic!("Could not write file `{:?}`: {}", &dockerfile_path, err));

    // Build docker image from the synapse root.
    std::process::Command::new("docker")
        .arg("build")
        .args(["--pull", "--no-cache"])
        .arg("-t")
        .arg(version.tag())
        .arg("-f")
        .arg(&dockerfile_path)
        .arg(&synapse_root)
        .output()
        .expect("Could not launch image rebuild");

    Ok(())
}

/// Bring things up.
pub fn up(
    version: SynapseVersion,
    script: &Option<Script>,
) -> Result<HashMap<&'static OsStr, OsString>, Error> {
    // FIXME: Up Synapse.
    // FIXME: If we have a token for an admin user, test it.
    // FIXME: Where should we store the token for the admin user? File storage? An embedded db?
    // FIXME: Note that we need to wait and retry, as bringing up Synapse can take a little time.
    // FIXME: If we have no token or the token is invalid, create an admin user.
    // FIXME: If the configuration states that we need to run an `up` script, run it.
    unimplemented!()
}

/// Bring things down.
pub fn down(
    version: SynapseVersion,
    script: &Option<DownScript>,
    status: Status,
) -> Result<(), Error> {
    match *script {
        None => {}
        Some(ref down_script) => {
            // First run on_failure/on_success.
            // Store errors for later.
            let result = match (status, down_script) {
                (
                    Status::Failure,
                    DownScript {
                        failure: Some(ref on_failure),
                        ..
                    },
                ) => on_failure.run(&*EMPTY_ENV),
                (
                    Status::Success,
                    DownScript {
                        success: Some(ref on_success),
                        ..
                    },
                ) => on_success.run(&*EMPTY_ENV),
                _ => Ok(()),
            };
            // Then run on_always.
            if let Some(ref on_always) = down_script.finally {
                on_always.run(&*EMPTY_ENV)?;
            }
            // Report any error from `on_failure` or `on_success`.
            result?
        }
    }
    // FIXME: Bring down Synapse.
    unimplemented!()
}

/// Run the testing script.
pub fn run(script: &Option<Script>) -> Result<(), Error> {
    if let Some(ref code) = script {
        let mut env = HashMap::new();
        // FIXME: Load the token, etc. from disk storage.
        // FIXME: Pass the real environment
        code.run(&env)?;
    }
    Ok(())
}
