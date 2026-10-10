BEAM17 handoff (from beam12-implementer)

## Branch state
- wp-BEAM17 in /home/mcloonan/redoubt/.worktrees/BEAM17, at main 38925184e (BEAM16's merge). CLEAN: nothing written, nothing committed. Only reading was done.
- Checkpoint and facts: /home/mcloonan/redoubt/.wash/local/BEAM17-checkpoint.md (read it first).

## Traps
- Never use AtomicU64 in the VM: it builds for rv32 (riscv32imac has no 64-bit atomics). Use AtomicUsize (or crate::sync::Lock).
- Questions to the orchestrator are capped at 2000 bytes; put detail in a .wash/local file.
- Every build/test through q; scratch under $REDOUBT_TMP/BEAM17 (= /home/mcloonan/redoubt/.tmp/BEAM17), never /tmp. beamlet-redoubt tests are host-clock: run with `q run --quiet`. ./test-shell beside other work can fail beamlet-redoubt console's an_end_of_input_already_waiting_ends_the_idle_that_takes_it (load); rerun with `q run --quiet -- ./test-shell`.
- difftest (all suites) has 6 atomvm failures on main (B34: test_binary_to_term, test_code_all_available_loaded, test_code_server_nifs, test_display_string, test_node, test_unicode); expect 518/524 or so; report them as B34's.
- `--sweep` cannot run with --prebuilt and needs a case pinning qemu_seed.
- The vm fixture tests/src/sized.erl loads no OTP modules (no lists:*): call natives directly (erts_internal:atomics_new/2, zlib:*_nif, persistent_term:put/2 is a native too). Rebuild: /home/mcloonan/redoubt/toolchains/otp-28.5.0.6/bin/erlc +deterministic -o vm/tests/fixtures vm/tests/src/sized.erl (from userland/otp).
- To show old-code failures without git stash: copy the new file to $REDOUBT_TMP, `git show HEAD:<path> > <path>`, test, copy back.
- Tier A: read in full every file you commit; the beamlet red reviews.

## Facts (verified in code)
1. On Redoubt max_ets_words is already a sixteenth of the VM's budget: userland/otp/redoubt/src/lib.rs:608-620 `limits()` sets max_heap_words and max_ets_words = pages*4096/LIMIT_SHARE/VM_WORD_BYTES (about 2.8 MB for a session of 11,008 pages). 1 GiB (2^27 words) is only vm::Limits::default (vm/src/vm.rs:30-60), the host CLI's.
2. ETS counts declared bytes only at insert: ets.rs `weigh(obj) = obj.words()`, and OwnedTerm::words() = heap.words(), which includes held_bytes (term/mod.rs ~500; a resource adds r.bytes() to held_bytes when pushed, term/mod.rs ~704). A resource resized after insert (zlib enqueue; screen resize) keeps its insert-time weight in ETS.
3. persistent_term (vm/src/bif/proc.rs:791 pt_put, MAX_PERSISTENT_TERMS = 65,536 terms) has no words limit. The value becomes a literal via System::make_literal (vm/src/vm.rs:1069) and a replaced value is NEVER freed (comment at proc.rs:802). Also pt_put_new at proc.rs:1197. persistent map is System.persistent (vm.rs:91); vm.rs:607 inserts the shell entry at start.
4. All resizes go through Ctx::resize_resource (vm/src/bif/mod.rs:824); callers: zlib.rs `counted`, screen/src/lib.rs:181, vm/tests/sized.rs. Resource::set_bytes is only called there.
5. Tables are deleted at bif/ets.rs:214 (ets:delete) and vm.rs:1460 (owner death); both drop the returned Table, so a Drop impl on Table catches deletion.

## Approved design and the orchestrator's conditions
- persistent_term: new Limits::max_persistent_words (host default 2^27 like ETS; on Redoubt the same sixteenth share in redoubt/src/lib.rs limits()). Count each put's words, declared bytes included, CUMULATIVELY (a replaced/erased value is not freed: code keeps it as a literal; state that on beamlet.md). A put past it is system_limit, as ETS's insert. Resources in a persistent value: track their live size too (resize adjusts).
- ETS (option a): keep the total INCREMENTALLY, no rescan on insert. The orchestrator requires: say the cost per insert and per resize.
- Measure the shell's ETS and persistent_term words at its prompt against the share and show the margin (beamlet's report_memory; or evaluate at the prompt on the fake kernel: ./shell --fake; check whether erlang:memory(ets)/persistent_term:info() exist on beamlet).
- Tests (VM host): a 128 MB atomics (erts_internal:atomics_new(16777216, _)) stored in persistent_term past the limit is system_limit; a zlib stream that grows inside ETS past max_ets_words is refused (next insert system_limit) or killed (your choice, say which); old-code failures shown. Use a Config with small limits (sized.rs builds Config { natives, ..Default::default() }; set limits.max_ets_words / max_persistent_words low).

## My sketch (not written)
- term/mod.rs Resource: add `ets: AtomicUsize` and `persistent: AtomicUsize` counts (how many ETS objects / persistent values hold it), with inc/dec/get methods; Heap::resources() iterating offheap Resource entries (OffHeap enum at term/mod.rs:165).
- ets.rs: weigh(obj) = term words only: obj.words() - obj.heap().held_bytes().div_ceil(8). Tables gets `held: Arc<AtomicUsize>` (bytes of resources in all tables, live); Table gets the same Arc (Table::new makes a fresh one; Tables::create swaps in the shared one while the table is empty). Table::enter(obj)/leave(obj): for each resource in obj's heap, inc/dec r.ets and add/sub r.bytes() to held. Call at insert (Set: old slot leaves, new enters; Bag: only if actually pushed; DuplicateBag: enters), remove, remove_object (each removed), replace (old leaves, new enters), clear (all leave), and impl Drop for Table (all leave). Tables::words() = sum of tables' term words + held.div_ceil(8). Cost: insert/remove O(resources in that object); resize O(1).
- persistent: System gets persistent_words (term words, cumulative) and a held counter; pt_put/pt_put_new: copy into a heap, words = heap.words() incl held; refuse system_limit if persistent_words + held/8 + words(excl held) > max_persistent_words; inc r.persistent for its resources (never dec: never freed).
- Ctx::resize_resource: take c.sys() first, then old = r.set_bytes(new); heap.resized(old,new); if r.ets>0 adjust sys.ets held by (new-old)*r.ets; same for persistent. Do set_bytes under the sys lock so ETS ops and resize don't interleave across schedulers.
- beamlet.md "Limits inside one VM": the ETS bullet (declared bytes count, live); a persistent_term bullet (max_persistent_words, cumulative, never freed); the Process memory bullet's residual sentence (now only a queued message's resource counts toward nobody, bounded by the mailbox limit); status list gains the new tests. Also redoubt lib.rs doc comment for limits().

## Gates (orchestrator's list)
beamlet-vm (cargo test -p beamlet-vm, q --cores 8, from userland/otp); ./test-shell; full difftest (cd userland/otp; . tools/env.sh; tools/difftest); prebuilt (make -f /home/mcloonan/redoubt/scripts/jobs.mk -C <worktree> prebuilt); then rv64 and rv32 beamlet-files, beamlet-boot, userland-boot, userland-read-only, beamlet-footprint; plus docs, formatting, unsafe-budget, no-cruft. Env for every shell: export PATH="$HOME/.cargo/bin:$PATH" RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains; unset MAKEFLAGS. Format with rustfmt +nightly --edition 2021 <files>.
Report as ASSIGNMENT RESULT (assignment 87da43deeefa0684594e3a9c10ab232c) with a report file .wash/local/BEAM17-report.md.
