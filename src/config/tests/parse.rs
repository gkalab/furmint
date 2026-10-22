use crate::config::parse_command;

#[test]
fn test_config_path_returns_some() {
    let path = crate::config::config_path();
    assert!(path.is_some(), "Config path should return Some(PathBuf)");
}

#[test]
fn test_parse_command() {
    assert_eq!(parse_command("vim"), ("vim".to_string(), vec![]));

    assert_eq!(
        parse_command("vim -u NONE"),
        (
            "vim".to_string(),
            vec!["-u".to_string(), "NONE".to_string()]
        )
    );

    assert_eq!(
        parse_command("\"C:/Program Files/vim.exe\" -v"),
        (
            "C:/Program Files/vim.exe".to_string(),
            vec!["-v".to_string()]
        )
    );

    assert_eq!(
        parse_command("C:/This/Path/Definitely/Does/Not/Exist/my_editor"),
        (
            "C:/This/Path/Definitely/Does/Not/Exist/my_editor".to_string(),
            vec![]
        )
    );
}
