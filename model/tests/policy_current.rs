//! Settled 117/125/153 observable policy contracts. No kernel-conformance or cryptographic claim.
use redoubt_model::policy::{confined_read_observation, connection_lineage, manifest};
use redoubt_model::serving::ConnectionShares;
use redoubt_model::steward::{AUDIT_DOMAIN, AuditKeyd, Denied, Steward};
use redoubt_steward::audit::Record;
use redoubt_steward::effect::{Answer, Refusal, Rendered};
use redoubt_steward::event::Content;
use redoubt_steward::inspect;

fn steward() -> Steward { Steward::new(&manifest(), 17, None).unwrap() }

/// A session's id from its login, each in a context of its own, so a test may hold several.
fn login(st: &mut Steward, name: &str, labels: &[u64]) -> u64 {
    static CONTEXTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let context = format!("c{}", CONTEXTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let key = if name == "alice" { 11 } else { 21 };
    match st.login(name, labels, &context, key) {
        Some(Answer::Session { id, .. }) => id,
        other => panic!("login {name} {labels:?}: {other:?}"),
    }
}

fn request(answer: Option<Answer>) -> u64 {
    match answer {
        Some(Answer::Request { id }) => id,
        other => panic!("not submitted: {other:?}"),
    }
}

/// An approval channel that opened, and the screen it shows for `id`.
fn channel(st: &mut Steward, name: &str, key: u64, id: u64) -> (u64, Option<Rendered>) {
    let (c, answer) = st.open_channel(name, key);
    assert_eq!(answer, Some(Answer::Ok));
    let screen = st.pending(c).into_iter().find(|r| r.id == id);
    (c, screen)
}

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
    let low = login(&mut st, "alice", &[]);
    assert!(matches!(st.start_agent(low, 1_000), Some(Answer::Lease { .. })));
    login(&mut st, "bob", &[]);
    assert_eq!(st.audit.len(), 3);
    assert!(st.audit_authentic());
    st.audit.reverse();
    st.audit_signatures.reverse();
    assert!(st.audit_authentic()); // per-record signatures do not detect reordering
    st.audit.pop();
    st.audit_signatures.pop();
    assert!(st.audit_authentic()); // nor omission of whole record/signature pairs
    st.audit.push(st.audit[0].clone());
    assert!(!st.audit_authentic()); // unsigned insertion detected
    st.audit.pop();
    st.audit[0] = st.audit[1].clone();
    assert!(!st.audit_authentic()); // edited record detected
}

#[test]
fn confined_read_down_and_owner_approved_one_item_snapshot_push() {
    for seed in 0..64u64 {
        let mut st = steward();
        let low = login(&mut st, "alice", &[]);
        let high = login(&mut st, "alice", &[7]);
        let bob = login(&mut st, "bob", &[]);
        let bytes = vec![seed as u8, 0, 255, b'x']; // push is bytes; not declassification's text cap
        st.write_item(low, &[], 10, bytes.clone()).unwrap();
        st.write_item(low, &[], 11, b"untouched".to_vec()).unwrap();
        assert_eq!(st.read_volume(high, &[], 10), Ok(bytes.clone()));
        st.confined = true;
        let read = st.read_volume(high, &[], 10);
        assert_eq!(read, Err(Denied::ReadDown));
        confined_read_observation(true, &[7], &[], &read).unwrap();
        // Deliberately replace the refusal with ordinary read-down success: the independent
        // oracle must reject this observable rule break, not merely mirror implementation.
        assert!(confined_read_observation(true, &[7], &[], &Ok(bytes.clone())).is_err());
        assert_eq!(st.read_volume(low, &[7], 99), Err(Denied::NotOwner));
        assert_eq!(st.write_item(high, &[], 10, vec![]), Err(Denied::NotOwner)); // no write down
        assert_eq!(st.open_channel("alice", 11).1, Some(Answer::Refused(Refusal::BadKey)));
        // The confined domain cannot trigger a push, nor a principal who does not own the target.
        let push = |item| Content::Push { source: 10, target: vec![7], item };
        assert_eq!(st.submit(high, push(99), ""), Some(Answer::Refused(Refusal::NotOwner)));
        assert_eq!(st.submit(bob, push(99), ""), Some(Answer::Refused(Refusal::NotOwner)));
        let id = request(st.submit(low, push(99), "pull in the data"));
        let other_target = request(st.submit(low, push(100), "pull in the data"));
        let (owner, screen) = channel(&mut st, "alice", 12, id);
        let hash = screen.expect("the owner's channel shows the push").hash;
        let target_hash = st.pending(owner).into_iter().find(|r| r.id == other_target).unwrap().hash;
        assert_ne!(hash, target_hash);
        assert_eq!(st.approve(owner, id, target_hash), Some(Answer::Refused(Refusal::HashMismatch)));
        st.write_item(low, &[], 10, b"changed after freezing".to_vec()).unwrap();
        assert_eq!(st.read_volume(high, &[7], 99), Ok(vec![]));
        let (other, shown) = channel(&mut st, "bob", 22, id);
        assert_eq!(shown, None);
        assert_eq!(st.approve(other, id, hash), Some(Answer::Refused(Refusal::Unknown)));
        // A second channel of the owner, which did not render it last, cannot answer it.
        let (second, _) = st.open_channel("alice", 12);
        assert_eq!(st.approve(second, id, hash), Some(Answer::Refused(Refusal::NotRendered)));
        let users_before = st.usage(1).unwrap();
        assert_eq!(st.approve(owner, id, hash), Some(Answer::Ok));
        assert_eq!(st.usage(1), Some(users_before));
        assert_eq!(st.read_volume(high, &[7], 99), Ok(bytes.clone()));
        assert_eq!(st.read_volume(high, &[7], 11), Ok(vec![])); // no batch or standing source path
        assert_eq!(st.approve(owner, id, hash), Some(Answer::Refused(Refusal::Unknown)));
        let pushed = st.audit.iter().find_map(|a| match a.record() {
            Record::Pushed { source, item, bytes, writer, .. } => {
                Some((a.domain().clone(), *source, *item, bytes.clone(), *writer))
            }
            _ => None,
        });
        let (domain, source, item, copied, writer) = pushed.expect("the push is audited");
        assert_eq!(domain.labels().as_slice(), &[7]);
        assert_eq!((source, item), (10, 99));
        assert_eq!(copied, bytes);
        assert_eq!(st.k.ghost.labels_at_creation.get(&writer), Some(&vec![7]));
        assert!(!st.k.budgets.contains_key(&writer));
        assert_eq!(st.k.budgets[&st.k.processes[&st.me.pid].budget].labels, Vec::<u64>::new());
        assert!(st.audit_authentic());
        assert!(!st.audit_view(&[]).iter().any(|a| matches!(a.record(), Record::Pushed { .. })));
    }
    eprintln!("push: 64 source snapshots; owner/target/hash/channel/single-consume/read-down checks passed");
}

#[test]
fn approve_and_deny_authenticate_direct_channels_before_any_effect() {
    for content in [
        Content::Declassify { labels: vec![7], item: 1 },
        Content::Agent { labels: vec![7], lease: 1_000 },
        Content::Note { what: "a pending request".into() },
    ] {
        let mut st = steward();
        let high = login(&mut st, "alice", &[7]);
        st.write_item(high, &[7], 1, b"private snapshot".to_vec()).unwrap();
        let id = request(st.submit(high, content, "direct-channel regression"));
        let (genuine, screen) = channel(&mut st, "alice", 12, id);
        let hash = screen.expect("the owner's channel shows it").hash;
        let valid_state = st.clone();
        let state = |st: &Steward| {
            let domains: Vec<_> = inspect::domains(&st.store).map(|(d, s)| (d.clone(), s.clone())).collect();
            (domains, st.audit.clone(), st.volumes.clone(), st.k.budgets.len())
        };
        for (name, key, refusal) in [
            ("alice", 11, Refusal::BadKey),  // login credential
            ("alice", 999, Refusal::BadKey), // unknown key
            ("alice", 22, Refusal::BadKey),  // another principal's approval key
            ("nobody", 12, Refusal::Unknown),
        ] {
            let (c, answer) = st.open_channel(name, key);
            assert_eq!(answer, Some(Answer::Refused(refusal)));
            let before = state(&st);
            assert_eq!(st.approve(c, id, hash), Some(Answer::Refused(Refusal::Unknown)));
            assert_eq!(state(&st), before, "refused approval changed pending/audit/resources");
            assert_eq!(st.deny(c, id), Some(Answer::Refused(Refusal::Unknown)));
            assert_eq!(state(&st), before, "refused denial changed pending/audit/resources");
        }
        // Independent controls show authenticated operations still consume their request.
        let mut approve = valid_state.clone();
        assert_eq!(approve.approve(genuine, id, hash), Some(Answer::Ok));
        let pending = |st: &Steward| inspect::domains(&st.store).any(|(_, s)| s.requests.contains_key(&id));
        assert!(!pending(&approve));
        let mut deny = valid_state;
        assert_eq!(deny.deny(genuine, id), Some(Answer::Ok));
        assert!(!pending(&deny));
    }
    // Keys keyd holds are fixed at boot: a manifest enrolling one as an approval key is refused.
    let mut held = manifest();
    held.keyd_keys.push(12);
    assert!(Steward::new(&held, 17, None).is_err());
}
