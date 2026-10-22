use crate::handlers::popup_ssh::parse_connection_string;

#[test]
fn test_parse_connection_string() {
    // Standard user@host
    let p1 = parse_connection_string("user@host").unwrap();
    assert_eq!(p1.user, "user");
    assert_eq!(p1.host, "host");
    assert_eq!(p1.path, None);

    // Host only (defaults to root)
    let p2 = parse_connection_string("host").unwrap();
    assert_eq!(p2.user, "root");
    assert_eq!(p2.host, "host");
    assert_eq!(p2.path, None);

    // Explicit root@host
    let p3 = parse_connection_string("root@host").unwrap();
    assert_eq!(p3.user, "root");
    assert_eq!(p3.host, "host");

    // Missing user with @ (e.g. @host -> root@host)
    let p4 = parse_connection_string("@host").unwrap();
    assert_eq!(p4.user, "root");
    assert_eq!(p4.host, "host");

    // Host with path
    let p5 = parse_connection_string("host:/home/user").unwrap();
    assert_eq!(p5.user, "root");
    assert_eq!(p5.host, "host");
    assert_eq!(p5.path, Some("/home/user".to_string()));

    // User@host with path
    let p6 = parse_connection_string("user@host:/tmp").unwrap();
    assert_eq!(p6.user, "user");
    assert_eq!(p6.host, "host");
    assert_eq!(p6.path, Some("/tmp".to_string()));

    assert!(parse_connection_string("").is_none());
}
