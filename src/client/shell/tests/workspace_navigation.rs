use super::*;

fn preview_remote(state: &mut ClientShellState) {
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"w");
    state.handle_input_bytes(b"\x1b[B");
}

#[test]
fn foreign_preview_blocks_rename_close_but_keeps_active_action_context() {
    let (mut state, remote_id) = state_with_remote();
    state.compose(100, 28).unwrap();
    preview_remote(&mut state);
    // Identical IDs must not allow rename/close to touch Local.
    for (key, confirm) in [('w', true), ('d', true), ('d', false)] {
        state.config.confirm_close = confirm;
        let outcome = state.handle_raw_events(vec![RawInputEvent::Key(
            crate::input::TerminalKey::new(KeyCode::Char(key), KeyModifiers::SHIFT),
        )]);
        assert!(outcome.actions.is_empty());
        assert!(outcome.requests.is_empty());
        assert!(state.overlay.is_none());
        assert_eq!(state.mode, ClientShellMode::Navigate);
        assert_eq!(
            state.navigate_workspace_id,
            state.navigation_target(&remote_id, "ws_1")
        );
    }
    // Workspace creation and worktree actions retain active-machine context.
    let mut remote = snapshot();
    remote.boot_id = "remote-boot".into();
    remote.workspaces[0].workspace_id = "remote-only".into();
    remote.focused_workspace_id = Some("remote-only".into());
    state.set_endpoint_snapshot(&remote_id, Box::new(remote));
    state.navigate_workspace_id = state.navigation_target(&remote_id, "remote-only");
    assert_eq!(state.workspace_action_id().as_deref(), Some("ws_1"));
    state.config.prompt_new_workspace_name = false;
    for action in [
        crate::input::KeybindAction::NewWorkspace,
        crate::input::KeybindAction::OpenWorktree,
    ] {
        let mut outcome = ClientShellInput::default();
        state.record_binding(crate::input::KeybindMatch::Action(action), &mut outcome);
        let [ClientShellAction::Endpoint {
            endpoint_id,
            request,
            ..
        }] = outcome.actions.as_slice()
        else {
            panic!("active endpoint request")
        };
        assert_eq!(endpoint_id, &ClientEndpointId::Local);
        match &request.method {
            crate::api::schema::Method::WorkspaceCreate(params) => {
                assert_eq!(params.source_workspace_id.as_deref(), Some("ws_1"))
            }
            crate::api::schema::Method::WorktreeList(params) => {
                assert_eq!(params.workspace_id.as_deref(), Some("ws_1"))
            }
            other => panic!("unexpected request {other:?}"),
        }
    }

    let cancel = state.handle_input_bytes(b"\x1b");
    assert!(cancel.actions.is_empty());
    assert!(cancel.requests.is_empty());
    assert!(state.navigate_workspace_id.is_none());
    assert_eq!(state.active_endpoint_id, ClientEndpointId::Local);

    // After activation, existing rename behavior is available on the remote machine.
    assert!(state.activate_endpoint_projection(&remote_id));
    state.handle_input_bytes(&[0x02]);
    state.handle_input_bytes(b"w");
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('w'),
        KeyModifiers::SHIFT,
    ))]);
    assert!(matches!(
        state.overlay.as_ref(),
        Some(ClientShellOverlay::Rename(_))
    ));
}

#[test]
fn foreign_preview_survives_local_updates_and_rejects_stale_enter() {
    for invalidation in [
        "offline",
        "disabled",
        "removed",
        "deleted",
        "boot",
        "generation",
    ] {
        let (mut state, remote_id) = state_with_remote();
        let mut remote = snapshot();
        remote.boot_id = "remote-boot".into();
        remote.focused_workspace_id = Some("remote-only".into());
        remote.workspaces[0].workspace_id = "remote-only".into();
        state.set_endpoint_snapshot_for_generation(&remote_id, 7, Box::new(remote.clone()));
        state.compose(100, 28).unwrap();
        preview_remote(&mut state);
        let selected = state.navigate_workspace_id.clone();
        assert_eq!(selected, state.navigation_target(&remote_id, "remote-only"));
        remote.revision += 1;
        state.set_endpoint_snapshot_for_generation(&remote_id, 7, Box::new(remote.clone()));
        assert_eq!(state.navigate_workspace_id, selected);
        assert!(state.navigation_target_valid(selected.as_ref().unwrap()));
        let mut local = snapshot();
        local.revision += 1;
        local.workspaces[0].label = "updated local".into();
        state.set_snapshot(Box::new(local));
        assert_eq!(state.navigate_workspace_id, selected);
        match invalidation {
            "offline" => state.set_endpoint_status(&remote_id, ClientEndpointStatus::Reconnecting),
            "disabled" => {
                let mut profile = remote_profile();
                profile.enabled = false;
                state.set_endpoint_catalog(&[profile]);
            }
            "removed" => state.set_endpoint_catalog(&[]),
            "deleted" => {
                remote.revision += 1;
                remote.workspaces.clear();
                state.set_endpoint_snapshot_for_generation(&remote_id, 7, Box::new(remote));
            }
            "boot" => {
                remote.boot_id = "restarted-remote".into();
                state.set_endpoint_snapshot_for_generation(&remote_id, 7, Box::new(remote));
            }
            "generation" => {
                state.cache_endpoint_snapshot_for_generation(&remote_id, 8, Box::new(remote))
            }
            _ => unreachable!(),
        }
        let enter = state.handle_input_bytes(b"\r");
        assert!(enter.actions.is_empty(), "{invalidation}");
        assert!(enter.requests.is_empty(), "{invalidation}");
        assert_eq!(state.active_endpoint_id, ClientEndpointId::Local);
        assert_eq!(state.mode, ClientShellMode::Navigate);
        assert!(!state.navigation_target_valid(state.navigate_workspace_id.as_ref().unwrap()));
        assert!(state.visible_endpoint_notice.is_some());
        // An explicit new movement can recover to a current, connected target.
        state.handle_input_bytes(b"\x1b[B");
        assert!(state.navigation_target_valid(state.navigate_workspace_id.as_ref().unwrap()));
    }
}

#[test]
fn navigation_uses_displayed_group_order_when_local_is_unavailable() {
    for cols in [100, 44] {
        let (mut state, remote_id) = state_with_remote();
        let mut remote = snapshot();
        remote.boot_id = "remote-boot".into();
        let mut other = remote.workspaces[0].clone();
        other.workspace_id = "other".into();
        remote.workspaces[0].worktree = Some(ClientShellWorktree {
            key: "repo".into(),
            label: "repo".into(),
            is_linked_worktree: false,
        });
        let mut child = remote.workspaces[0].clone();
        child.workspace_id = "child".into();
        child.worktree.as_mut().unwrap().is_linked_worktree = true;
        remote.workspaces.extend([other, child]);
        state.set_endpoint_snapshot(&remote_id, Box::new(remote));
        state.set_endpoint_status(&ClientEndpointId::Local, ClientEndpointStatus::Reconnecting);
        state.select_unavailable_local();
        state.sidebar_collapsed = true;
        state.compose(cols, 18).unwrap();
        state.handle_input_bytes(&[0x02]);
        state.handle_input_bytes(b"w");
        for id in ["ws_1", "child", "other"] {
            state.handle_input_bytes(b"\x1b[B");
            assert_eq!(
                state.navigate_workspace_id,
                state.navigation_target(&remote_id, id)
            );
            state.compose(cols, 18).unwrap();
            assert!(
                state
                    .hits
                    .workspaces
                    .iter()
                    .any(|hit| hit.endpoint_id == remote_id && hit.workspace_id == id),
                "{id} must stay visible at {cols} columns"
            );
        }
        let enter = state.handle_input_bytes(b"\r");
        assert!(
            matches!(enter.actions.as_slice(), [ClientShellAction::ActivateEndpoint { endpoint_id, target: Some(ClientEndpointFocusTarget::Workspace(id)) }]
        if endpoint_id == &remote_id && id == "other")
        );
    }
}

#[test]
fn active_preview_is_not_retargeted_by_deletion_or_reboot() {
    for invalidation in ["deleted", "boot", "generation"] {
        let (mut state, _) = state_with_remote();
        let mut local = snapshot();
        let mut second = local.workspaces[0].clone();
        second.workspace_id = "ws_2".into();
        second.focused = false;
        local.workspaces.push(second);
        state.set_endpoint_snapshot_for_generation(
            &ClientEndpointId::Local,
            7,
            Box::new(local.clone()),
        );
        state.compose(100, 28).unwrap();
        preview_remote(&mut state); // Down selects the second Local workspace here.
        let selected = state.navigate_workspace_id.clone();
        assert_eq!(
            selected,
            state.navigation_target(&ClientEndpointId::Local, "ws_2")
        );
        if invalidation == "boot" {
            local.boot_id = "new-local-boot".into();
        } else if invalidation == "deleted" {
            local.revision += 1;
            local.workspaces.pop();
        }
        let generation = if invalidation == "generation" { 8 } else { 7 };
        state.set_endpoint_snapshot_for_generation(
            &ClientEndpointId::Local,
            generation,
            Box::new(local),
        );
        assert_eq!(state.navigate_workspace_id, selected);
        let enter = state.handle_input_bytes(b"\r");
        assert!(enter.actions.is_empty());
        assert!(enter.requests.is_empty());
        assert_eq!(state.mode, ClientShellMode::Navigate);
        assert!(state.visible_endpoint_notice.is_some());
        for (key, confirm) in [('w', true), ('d', true), ('d', false)] {
            state.config.confirm_close = confirm;
            let outcome = state.handle_raw_events(vec![RawInputEvent::Key(
                crate::input::TerminalKey::new(KeyCode::Char(key), KeyModifiers::SHIFT),
            )]);
            assert!(outcome.actions.is_empty());
            assert!(outcome.requests.is_empty());
            assert!(state.overlay.is_none());
            assert_eq!(state.mode, ClientShellMode::Navigate);
        }
        assert_eq!(state.workspace_action_id().as_deref(), Some("ws_1"));
    }
}

#[test]
fn aggregate_navigation_reveals_overflow_and_preserves_order() {
    for (collapsed, cols) in [(true, 100), (false, 100), (false, 44)] {
        let (mut state, remote_id) = state_with_remote();
        let mut remote = snapshot();
        remote.boot_id = "remote-boot".into();
        for number in 2..=15 {
            let mut workspace = remote.workspaces[0].clone();
            workspace.workspace_id = format!("ws_{number}");
            workspace.number = number;
            workspace.focused = false;
            remote.workspaces.push(workspace);
        }
        state.set_endpoint_snapshot(&remote_id, Box::new(remote));
        state.sidebar_collapsed = collapsed;
        state.collapsed_endpoints.insert(remote_id.clone());
        state.compose(cols, 18).unwrap();
        state.handle_input_bytes(&[0x02]);
        state.handle_input_bytes(b"w");
        for number in 1..=15 {
            let movement = state.handle_input_bytes(b"\x1b[B");
            assert!(movement.actions.is_empty());
            assert!(movement.requests.is_empty());
            let workspace_id = format!("ws_{number}");
            assert_eq!(
                state.navigate_workspace_id,
                state.navigation_target(&remote_id, &workspace_id)
            );
            state.compose(cols, 18).unwrap();
            let visible = if cols == 44 {
                state.hits.mobile_targets.iter().any(|(_, target)| matches!(target,
                    ClientMobileTarget::Workspace { endpoint_id, workspace_id: id } if endpoint_id == &remote_id && id == &workspace_id))
            } else {
                state
                    .hits
                    .workspaces
                    .iter()
                    .any(|hit| hit.endpoint_id == remote_id && hit.workspace_id == workspace_id)
            };
            assert!(visible, "{workspace_id} collapsed={collapsed} cols={cols}");
        }
        assert!(!state.collapsed_endpoints.contains(&remote_id));
        state.handle_input_bytes(b"\x1b[B");
        let (endpoint_id, workspace_id) = if cols == 44 {
            (&remote_id, "ws_15")
        } else {
            (&ClientEndpointId::Local, "ws_1")
        };
        assert_eq!(
            state.navigate_workspace_id,
            state.navigation_target(endpoint_id, workspace_id)
        );
        state.handle_input_bytes(b"\x1b[A");
        let workspace_id = if cols == 44 { "ws_14" } else { "ws_15" };
        assert_eq!(
            state.navigate_workspace_id,
            state.navigation_target(&remote_id, workspace_id)
        );
        state.set_endpoint_status(&remote_id, ClientEndpointStatus::Reconnecting);
        state.handle_input_bytes(b"\x1b[B");
        assert_eq!(
            state.navigate_workspace_id,
            state.navigation_target(&ClientEndpointId::Local, "ws_1")
        );
    }
}
