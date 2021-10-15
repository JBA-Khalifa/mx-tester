use std::convert::TryFrom;

use mx_tester::{self, registration::User, *};

/// A trivial test that checks that steps build, up and down can be executed
/// with the default configuration.
#[tokio::test]
async fn test_default_config() {
    let config = Config::default();
    let container_config = ContainerConfig::try_from(&config)
        .expect("Should be able to convert the config without issue.");
    mx_tester::build(&config.modules, &SynapseVersion::ReleasedDockerImage)
        .expect("Failed in step `build`");
    mx_tester::up(
        &SynapseVersion::ReleasedDockerImage,
        &config,
        &container_config,
        &config.homeserver_config,
    )
    .await
    .expect("Failed in step `up`");
    mx_tester::down(
        &SynapseVersion::ReleasedDockerImage,
        &config.down,
        Status::Manual,
    )
    .expect("Failed in step `down`");
}

#[tokio::test]
async fn test_create_users() {
    use matrix_sdk::HttpSend;

    // Setup with two users.
    let admin = User::builder()
        .admin(true)
        .localname(format!("admin-{}", uuid::Uuid::new_v4()))
        .build();

    let regular_user = User::builder()
        .localname(format!("regular-user-{}", uuid::Uuid::new_v4()))
        .build();
    let config = Config {
        users: vec![admin.clone(), regular_user.clone()],
        ..Config::default()
    };
    let container_config = ContainerConfig::try_from(&config)
        .expect("Should be able to convert the config without issue.");
    mx_tester::build(&config.modules, &SynapseVersion::ReleasedDockerImage)
        .expect("Failed in step `build`");
    mx_tester::up(
        &SynapseVersion::ReleasedDockerImage,
        &config,
        &container_config,
        &config.homeserver_config,
    )
    .await
    .expect("Failed in step `up`");

    // Now attempt to login as our users.
    let homeserver_url = reqwest::Url::parse(&config.homeserver_config.public_baseurl).unwrap();
    let admin_client = matrix_sdk::Client::new(homeserver_url.clone()).unwrap();
    admin_client
        .login(&admin.localname, &admin.localname, None, None)
        .await
        .expect("Could not login as admin");
    // FIXME: Check that we can use an admin API.

    let regular_user_client = matrix_sdk::Client::new(homeserver_url.clone()).unwrap();
    regular_user_client
        .login(&regular_user.localname, &regular_user.localname, None, None)
        .await
        .expect("Could not login as regular user");
    mx_tester::down(
        &SynapseVersion::ReleasedDockerImage,
        &config.down,
        Status::Manual,
    )
    .expect("Failed in step `down`");
}
