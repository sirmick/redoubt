//! Settled 117/125/153 observable policy contracts. No kernel-conformance or cryptographic claim.
use redoubt_model::policy::{confined_read_observation, connection_lineage, manifest};
use redoubt_model::steward::{AUDIT_DOMAIN, Audit, AuditKeyd, Channel, ConnectionShares, Denied, Steward};

fn steward() -> Steward { Steward::new(&manifest(), 17, None).unwrap() }

#[test]
fn delegated_badges_share_transitively_and_disconnect_refunds_only_their_state() {
    let mut shares = ConnectionShares::new(4).unwrap();
    shares.grant(1, 7, vec![], None).unwrap();
    shares.grant(2, 7, vec![], None).unwrap(); // independently authorized sponsor/agent share
    shares.grant(3, 7, vec![], Some(1)).unwrap();
    shares.grant(4, 7, vec![], Some(3)).unwrap();
    shares.admit(1).unwrap();
    shares.admit(4).unwrap();
    assert_eq!(shares.admit(3), Err(Denied::Cap));
    shares.admit(2).unwrap();
    shares.admit(2).unwrap();
    assert_eq!(shares.admit(2), Err(Denied::Cap));
    shares.disconnect(4).unwrap();
    shares.admit(3).unwrap();
    assert_eq!(shares.admit(3), Err(Denied::Cap));
    assert!(shares.grant(4, 7, vec![], None).is_err()); // badges never reused
    assert!(ConnectionShares::new(1).is_err());
}

#[test]
fn changed_buckets_and_system_badges_do_not_debit_another_share() {
    let mut shares = ConnectionShares::new(2).unwrap();
    for (badge, account, labels, parent) in [
        (1, 7, vec![], None),
        (2, 7, vec![9], Some(1)),
        (3, 8, vec![], Some(1)),
        (4, 0, vec![], None),
        (5, 0, vec![], Some(4)),
    ] {
        shares.grant(badge, account, labels, parent).unwrap();
        shares.admit(badge).unwrap();
        shares.admit(badge).unwrap();
        assert_eq!(shares.admit(badge), Err(Denied::Cap));
    }
}

#[test]
fn random_lineage_sequences_and_deliberate_rule_break() {
    for seed in 0..256 {
        connection_lineage(seed, 128, false).unwrap();
    }
    assert!(
        (0..32).any(|seed| connection_lineage(seed, 128, true).is_err()),
        "oracle accepted independent shares for self-minted badges"
    );
    eprintln!("connection_lineage: 256 sequences x 128 operations; deliberate lineage break detected");
}

#[test]
fn audit_authority_binds_purpose_signer_domain_length_and_every_byte() {
    let keyd = AuditKeyd::new(1);
    let record = b"one complete audit record";
    let sig = keyd.sign("audit", record).unwrap();
    let mut expected = b"redoubt.audit.v1\0".to_vec();
    expected.extend_from_slice(&(record.len() as u64).to_le_bytes());
    expected.extend_from_slice(record);
    assert_eq!(sig.preimage(), expected);
    assert!(sig.verify(1, "audit", record));
    assert!(!sig.verify(2, "audit", record));
    assert!(!sig.verify(1, "bundle", record));
    assert_eq!(keyd.sign("bundle", record), Err(Denied::BadKey));
    for index in 0..record.len() {
        let mut changed = record.to_vec();
        changed[index] ^= 1;
        assert!(!sig.verify(1, "audit", &changed));
    }
    assert!(!sig.verify(1, "audit", &record[..record.len() - 1]));
    // Asking keyd to sign a forged domain/length as data cannot override its own domain.
    for index in [0, AUDIT_DOMAIN.len()] {
        let mut forged = expected.clone();
        forged[index] ^= 1;
        assert!(!keyd.sign("audit", &forged).unwrap().verify(1, "audit", record));
    }
}

#[test]
fn every_existing_audit_entry_is_signed_but_no_chain_is_claimed() {
    let mut st = steward();
    let low = st.login("alice", None, 11).unwrap();
    st.start_agent(low, 1_000).unwrap();
    assert!(st.audit_authentic());
    st.audit.reverse();
    st.audit_signatures.reverse();
    assert!(st.audit_authentic()); // per-record signatures do not detect reordering
    st.audit.pop();
    st.audit_signatures.pop();
    assert!(st.audit_authentic()); // nor omission of whole record/signature pairs
    st.audit.push(Audit::LeaseEnded { session: 0, by: 0 });
    assert!(!st.audit_authentic()); // unsigned insertion detected
    st.audit.pop();
    st.audit[0] = Audit::LeaseEnded { session: 123, by: 456 };
    assert!(!st.audit_authentic()); // edited record detected
}

#[test]
fn confined_read_down_and_owner_approved_one_item_snapshot_push() {
    for seed in 0..64u64 {
        let mut st = steward();
        let low = st.login("alice", None, 11).unwrap();
        let high = st.login("alice", Some(7), 11).unwrap();
        let bytes = vec![seed as u8, 0, 255, b'x']; // push is bytes; not declassification's text cap
        st.write_unlabelled(low, 10, bytes.clone()).unwrap();
        st.write_unlabelled(low, 11, b"untouched".to_vec()).unwrap();
        assert_eq!(st.read_volume(high, None, 10), Ok(bytes.clone()));
        st.confined = true;
        let read = st.read_volume(high, None, 10);
        assert_eq!(read, Err(Denied::ReadDown));
        confined_read_observation(true, &[7], None, &read).unwrap();
        // Deliberately replace the refusal with ordinary read-down success: the independent
        // oracle must reject this observable rule break, not merely mirror implementation.
        assert!(confined_read_observation(true, &[7], None, &Ok(bytes.clone())).is_err());
        assert_eq!(st.read_volume(low, Some(7), 99), Err(Denied::NotOwner));
        assert_eq!(st.write_unlabelled(high, 10, vec![]), Err(Denied::Labelled));
        assert_eq!(st.open_approval("alice", 11), Err(Denied::BadKey));
        // A requester cannot substitute session-held credentials for an owner action.
        assert_eq!(
            st.prepare_push(Channel { principal: 0, key: 11 }, 10, 99, 7, vec![7]),
            Err(Denied::BadKey)
        );
        let owner = st.open_approval("alice", 12).unwrap();
        let other = st.open_approval("bob", 22).unwrap();
        assert_eq!(st.prepare_push(other, 10, 99, 7, vec![]), Err(Denied::NotOwner));
        let id = st.prepare_push(owner, 10, 99, 7, vec![7]).unwrap();
        let hash = st.push_requests[&id].hash;
        let different_target = st.prepare_push(owner, 10, 100, 7, vec![7]).unwrap();
        let target_hash = st.push_requests[&different_target].hash;
        assert_ne!(hash, target_hash);
        assert_eq!(st.approve_push(owner, id, target_hash), Err(Denied::HashMismatch));
        st.write_unlabelled(low, 10, b"changed after freezing".to_vec()).unwrap();
        assert_eq!(st.read_volume(high, Some(7), 99), Ok(vec![]));
        assert_eq!(st.approve_push(other, id, hash), Err(Denied::NotApprover));
        assert_eq!(st.approve_push(owner, id, hash ^ 1), Err(Denied::HashMismatch));
        let users_before = st.usage(1).unwrap();
        st.approve_push(owner, id, hash).unwrap();
        assert_eq!(st.usage(1), Some(users_before));
        assert_eq!(st.read_volume(high, Some(7), 99), Ok(bytes.clone()));
        assert_eq!(st.read_volume(high, Some(7), 11), Ok(vec![])); // no batch or standing source path
        assert_eq!(st.approve_push(owner, id, hash), Err(Denied::NoSuchRequest));
        let Audit::Pushed { writer, labels, source, target, bytes: copied, .. } = st.audit.last().unwrap()
        else {
            panic!("missing push audit")
        };
        assert_eq!(labels, &[7]);
        assert_eq!((*source, *target), (10, 99));
        assert_eq!(copied, &bytes);
        assert_eq!(st.k.ghost.labels_at_creation.get(writer), Some(&vec![7]));
        assert!(!st.k.budgets.contains_key(writer));
        assert_eq!(st.k.budgets[&st.k.processes[&st.me.pid].budget].labels, Vec::<u64>::new());
        assert!(st.audit_authentic());
        assert!(!st.audit_view(&[]).iter().any(|a| matches!(a, Audit::Pushed { .. })));
    }
    eprintln!("push: 64 source snapshots; owner/target/hash/single-consume/read-down checks passed");
}

#[test]
fn approve_and_deny_authenticate_direct_channels_before_any_effect() {
    use redoubt_model::steward::Content;
    for content in [
        Content::Declassify { label: 7, item: 1 },
        Content::AgentWithLabel { label: 7, lease: 1_000 },
        Content::Note { what: "a pending request".into() },
    ] {
        let mut st = steward();
        let high = st.login("alice", Some(7), 11).unwrap();
        st.write_item(high, 7, 1, b"private snapshot".to_vec()).unwrap();
        let id = st.submit(high, content, "direct-channel regression").unwrap();
        let hash = st.requests[&id].hash;
        let genuine = st.open_approval("alice", 12).unwrap();
        let valid_state = st.clone();
        for (channel, error) in [
            (Channel { principal: 0, key: 11 }, Denied::BadKey), // login credential
            (Channel { principal: 0, key: 999 }, Denied::BadKey), // unknown key
            (Channel { principal: 0, key: 22 }, Denied::BadKey), // another principal's approval key
            (Channel { principal: usize::MAX, key: 12 }, Denied::UnknownPrincipal),
        ] {
            let before = format!("{st:?}");
            assert_eq!(st.approve(channel, id, hash), Err(error));
            assert_eq!(format!("{st:?}"), before, "refused approval changed pending/audit/resources");
            assert_eq!(st.deny(channel, id), Err(error));
            assert_eq!(format!("{st:?}"), before, "refused denial changed pending/audit/resources");
            assert!(st.requests.contains_key(&id));
            assert!(st.declassified.is_empty());
        }
        // A channel minted legitimately before keyd acquired its key must be revalidated;
        // possession of this old public value cannot bypass the current key separation rule.
        st.keyd_add(genuine.key);
        let before = format!("{st:?}");
        assert_eq!(st.approve(genuine, id, hash), Err(Denied::BadKey));
        assert_eq!(format!("{st:?}"), before);
        assert_eq!(st.deny(genuine, id), Err(Denied::BadKey));
        assert_eq!(format!("{st:?}"), before);
        assert!(st.requests.contains_key(&id));
        assert!(st.declassified.is_empty());

        // Independent controls show authenticated operations still consume their request.
        let mut approve = valid_state.clone();
        approve.approve(genuine, id, hash).unwrap();
        assert!(!approve.requests.contains_key(&id));
        let mut deny = valid_state;
        deny.deny(genuine, id).unwrap();
        assert!(!deny.requests.contains_key(&id));
    }
}
