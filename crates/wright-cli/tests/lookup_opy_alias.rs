#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn lookup(query: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "wright-lookup-opy-alias-{}-{query}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let provider = dir.join("provider");
    std::fs::write(
        &provider,
        r##"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    request = json.loads(line)
    method = request['method']
    if method == 'lpp/initialize':
        result = {'protocolVersion': request['params']['protocolVersion'],
                  'serverInfo': {'name': 'fake', 'version': '0'},
                  'languages': [{'id': 'opy', 'extensions': ['opy']}],
                  'capabilities': {'check': True, 'compile': True, 'reconstruct': False,
                                   'symbols': False, 'definition': False, 'references': False,
                                   'rename': False, 'editValidation': False,
                                   'projectLoading': False, 'lookup': True}}
    elif method == 'lpp/lookup':
        result = {'entries': [
            {'identity': 'opy:callable/getHealth', 'kind': 'value', 'spelling': 'getHealth',
             'displayName': 'Health', 'description': 'The current health of a player.',
             'aliases': ['health'],
             'callable': {'parameters': [
                 {'name': 'player', 'type': 'Player', 'required': True,
                  'description': 'The player to read.'}]}}]}
    else:
        result = {}
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
"##,
    )
    .unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(["lookup", query, "--language", "opy", "--opy-provider"])
        .arg(&provider)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn opy_card_shows_descriptions_and_names_the_canonical_spelling_of_an_alias() {
    let canonical = "value getHealth\n  The current health of a player.\n  getHealth(player: Player)\n  player: The player to read.\n";
    assert_eq!(lookup("getHealth"), canonical);
    assert_eq!(
        lookup("health"),
        canonical.replacen("\n", "\n  health is an alias of getHealth\n", 1)
    );
}
