mod common;
use common::Workspace;
use numera_image_gen_mcp::cli::{
    http_settings,
    menu::{MenuAction, step},
};
use std::{collections::HashMap, io::Cursor};

#[test]
fn start_exit_and_eof_are_explicit_menu_actions_without_a_background_process() {
    let workspace = Workspace::new();
    let env = HashMap::new();
    let mut output = Vec::new();
    assert_eq!(
        step(&workspace.0, &env, &mut Cursor::new(b"\n"), &mut output).unwrap(),
        MenuAction::StartHttp { port: 1969 }
    );
    assert_eq!(
        step(&workspace.0, &env, &mut Cursor::new(b"1\n"), &mut output).unwrap(),
        MenuAction::StartHttp { port: 1969 }
    );
    assert_eq!(
        step(&workspace.0, &env, &mut Cursor::new(b"4\n"), &mut output).unwrap(),
        MenuAction::Exit
    );
    assert_eq!(
        step(&workspace.0, &env, &mut Cursor::new(b""), &mut output).unwrap(),
        MenuAction::Exit
    );
    let text = String::from_utf8(output).unwrap();
    for label in [
        "1. Start HTTP server",
        "2. Change port",
        "3. Show connection guidance",
        "4. Exit",
    ] {
        assert!(text.contains(label));
    }
    assert!(
        !workspace.0.join("config/http-token").exists(),
        "menu selection itself must not generate a token or start HTTP"
    );
}

#[test]
fn port_edits_persist_only_after_valid_input_and_guidance_never_displays_credentials() {
    let workspace = Workspace::new();
    let env = HashMap::new();
    let mut output = Vec::new();
    assert_eq!(
        step(
            &workspace.0,
            &env,
            &mut Cursor::new(b"2\n2345\n"),
            &mut output
        )
        .unwrap(),
        MenuAction::Continue
    );
    assert_eq!(http_settings::port(&workspace.0, &[], &env).unwrap(), 2345);
    for input in [b"2\n0\n".as_slice(), b"2\nnot-a-port\n", b"2\n\n", b"9\n"] {
        assert_eq!(
            step(&workspace.0, &env, &mut Cursor::new(input), &mut output).unwrap(),
            MenuAction::Continue
        );
        assert_eq!(http_settings::port(&workspace.0, &[], &env).unwrap(), 2345);
    }
    assert_eq!(
        step(&workspace.0, &env, &mut Cursor::new(b"2\n"), &mut output).unwrap(),
        MenuAction::Exit
    );
    let secret = "synthetic-menu-secret-0123456789abcdef";
    let env = HashMap::from([("NUMERA_HTTP_TOKEN".into(), secret.into())]);
    assert_eq!(
        step(&workspace.0, &env, &mut Cursor::new(b"3\n"), &mut output).unwrap(),
        MenuAction::Continue
    );
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("http://127.0.0.1:2345/mcp"));
    assert!(text.contains("NUMERA_HTTP_TOKEN"));
    assert!(!text.contains(secret));
    assert!(!text.contains("Bearer synthetic"));
    assert!(!workspace.0.join("config/http-token").exists());
}

#[test]
fn bounded_invalid_menu_input_does_not_get_interpreted_as_a_start_command() {
    let workspace = Workspace::new();
    let mut input = Cursor::new(vec![b'1'; 1025]);
    let mut output = Vec::new();
    assert!(step(&workspace.0, &HashMap::new(), &mut input, &mut output).is_err());
    assert!(!workspace.0.join("config/http-token").exists());
}
