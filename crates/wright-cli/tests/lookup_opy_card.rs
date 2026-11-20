#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn opy_card_uses_type_names_for_enum_domains() {
    let dir = std::env::temp_dir().join(format!("wright-lookup-opy-{}", std::process::id()));
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
        result = {'protocolVersion': '1.5', 'serverInfo': {'name': 'fake', 'version': '0'},
                  'languages': [{'id': 'opy', 'extensions': ['opy']}],
                  'capabilities': {'check': True, 'compile': True, 'reconstruct': False,
                                   'symbols': False, 'definition': False, 'references': False,
                                   'rename': False, 'editValidation': False,
                                   'projectLoading': False, 'lookup': True}}
    elif method == 'lpp/lookup':
        domain = {'domain': 'opy:enum/HudPosition', 'members': ['LEFT', 'RIGHT']}
        result = {'entries': [
            {'identity': 'opy:callable/hudText', 'kind': 'action', 'spelling': 'hudText',
             'displayName': 'HUD Text', 'callable': {'parameters': [
                 {'name': 'location', 'type': 'HudPosition', 'required': False,
                  'default': 'HudPosition.LEFT', 'enum': domain},
                 {'name': 'other', 'type': 'HudPosition', 'required': False,
                  'default': 'HudPosition.RIGHT', 'enum': domain}]}},
            {'identity': 'opy:callable/destroyHudText', 'kind': 'action',
             'spelling': 'destroyHudText', 'displayName': 'Destroy HUD Text',
             'callable': {'parameters': []}}]}
    else:
        result = {}
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
"##,
    )
    .unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args(["lookup", "hudText", "--language", "opy", "--opy-provider"])
        .arg(&provider)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        text,
        "action hudText\n  hudText(location: HudPosition=HudPosition.LEFT, other: HudPosition=HudPosition.RIGHT)\n  HudPosition: LEFT | RIGHT\naction destroyHudText()\n"
    );
}
