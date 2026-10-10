# The guards the tables name (docs/servers/steward.md, "Guards and effects"), for the Elixir
# reference. Each reads the transition's context and changes nothing: :ok, or {:error, reason}
# with the refusal a `!guard` row gives.
defmodule Redoubt.Steward.Guards do
  alias Redoubt.Steward

  @pending_cap 4
  @declassify_max 256
  @blame_count 3
  @blame_window 10 * 60 * 1_000_000
  @max_lease 24 * 3600 * 1_000_000

  def pending_cap_value, do: @pending_cap
  def blame_count, do: @blame_count
  def blame_window_us, do: @blame_window

  defp ok_if(true, _), do: :ok
  defp ok_if(false, why), do: {:error, why}

  def state(cx), do: cx.store.domains[cx.domain]
  def session(cx), do: state(cx).sessions[cx.id]
  def lease(cx), do: state(cx).leases[cx.id]
  def request(cx), do: state(cx).requests[cx.id]
  def crossing(cx), do: state(cx).crossings[cx.id]

  defp principal(cx, p), do: Enum.at(cx.store.fixed.principals, p)
  defp labels(cx), do: elem(cx.domain, 1)

  # The labels a request asks for beyond its own; nil if they are not a label set.
  defp asked(%{content: {:note, _}}), do: []
  defp asked(%{content: {:agent, l, _}}), do: Steward.labels(l)
  defp asked(%{content: {:declassify, l, _}}), do: Steward.labels(l)
  defp asked(%{content: {:push, _, l, _}}), do: Steward.labels(l)

  # R35: a login uses one of its principal's login keys, never one keyd holds.
  def login_key(cx) do
    case session(cx) do
      nil ->
        {:error, :Unknown}

      s ->
        p = principal(cx, s.principal)
        ok_if(s.key in p.login_keys and s.key not in cx.store.fixed.keyd, :BadKey)
    end
  end

  # R35: an approval channel uses one of its principal's approval keys, never anyone's login key
  # or one keyd holds.
  def approval_key(cx) do
    case cx.store.channels[cx.id] do
      nil ->
        {:error, :Unknown}

      c ->
        p = principal(cx, c.principal)
        login = Enum.any?(cx.store.fixed.principals, &(c.key in &1.login_keys))
        ok_if(c.key in p.approval_keys and not login and c.key not in cx.store.fixed.keyd, :BadKey)
    end
  end

  # A vault login, a labelled agent, a declassification or a push needs the labels' owner, from
  # the manifest's owned labels. A login is refused as a wrong key is: no enumeration.
  def owns_labels(cx) do
    who =
      case cx.kind do
        :session ->
          case session(cx) do
            nil -> {:error, :Unknown}
            s -> {s.principal, []}
          end

        :request ->
          case request(cx) do
            nil ->
              {:error, :Unknown}

            r ->
              case asked(r) do
                nil -> {:error, :NotOwner}
                wanted -> {r.principal, wanted}
              end
          end

        _ ->
          {:error, :Unknown}
      end

    case who do
      {:error, _} = e ->
        e

      {p, wanted} ->
        owned = principal(cx, p).owned
        refusal = if cx.kind == :session, do: :BadKey, else: :NotOwner
        ok_if(Steward.includes?(owned, labels(cx)) and Steward.includes?(owned, wanted), refusal)
    end
  end

  # A login that would make a context finds fewer than its principal's cap of live sessions in its
  # domain, contexts and the console's; nothing is evicted. The console's own session is not capped.
  def under_cap(cx) do
    case session(cx) do
      nil ->
        {:error, :Unknown}

      %{context: nil} ->
        :ok

      s ->
        live =
          state(cx).sessions
          |> Map.values()
          |> Enum.count(&(&1.id != s.id and &1.state != :ending))

        ok_if(live < principal(cx, s.principal).contexts.max, :Cap)
    end
  end

  # A detached context has been idle for its principal's bound since its clock started.
  def idle_due(cx) do
    case session(cx) do
      nil ->
        {:error, :Unknown}

      s ->
        bound = principal(cx, s.principal).contexts.idle_secs * 1_000_000
        ok_if(s.idle != nil and cx.event.now - s.idle >= bound, :Unknown)
    end
  end

  # The session is a context, not the console's.
  def is_context(cx) do
    case session(cx) do
      %{context: c} when c != nil -> :ok
      _ -> {:error, :Unknown}
    end
  end

  # R79: a context is one session at a time: the name a login gives is no other session's of its
  # domain, unless that one is already ending. The console's session (context nil) is no context.
  def context_free(cx) do
    case session(cx) do
      nil ->
        {:error, :Unknown}

      %{context: nil} ->
        :ok

      s ->
        taken =
          state(cx).sessions
          |> Map.values()
          |> Enum.any?(&(&1.id != s.id and &1.state != :ending and &1.context == s.context))

        ok_if(not taken, :InUse)
    end
  end

  # A labelled session or agent starts nothing.
  def caller_unlabelled(cx), do: ok_if(labels(cx) == [], :Labelled)

  # R40: nothing new starts in a locked-out domain until its window passes; for an approval,
  # the domain the grant would start a lease in. A request that starts no lease is not held.
  def not_locked(cx) do
    blame = if cx.kind == :request and cx.grant != nil, do: cx.grant.blame, else: state(cx).blame

    if cx.kind == :request and not match?(%{content: {:agent, _, _}}, request(cx)) do
      :ok
    else
      ok_if(cx.event.now >= blame.until, :LockedOut)
    end
  end

  # R40: this blame is the third within the window.
  def blame_window(cx) do
    now = cx.event.now
    recent = Enum.count(state(cx).blame.times, &(max(now - &1, 0) < @blame_window))
    ok_if(recent + 1 >= @blame_count, :Unknown)
  end

  defp live?(r, cx), do: r.id != cx.id and r.state not in Steward.Gen.Request.finals()

  # The pending cap per domain.
  def pending_cap(cx) do
    pending = state(cx).requests |> Map.values() |> Enum.count(&live?(&1, cx))
    ok_if(pending < @pending_cap, :Cap)
  end

  # A fair share of the cap per session or agent: the cap divided among the domain's sessions
  # and agents, at least one.
  def fair_share(cx) do
    case request(cx) do
      nil ->
        {:error, :Unknown}

      r ->
        st = state(cx)
        mine = st.requests |> Map.values() |> Enum.count(&(live?(&1, cx) and &1.by == r.by))
        live = map_size(st.sessions) + map_size(st.leases)
        ok_if(mine < max(div(@pending_cap, max(live, 1)), 1), :Cap)
    end
  end

  # R39: a lease of more than 0 and at most a day, asked for directly or in a request.
  def lease_bounded(cx) do
    asked =
      case {cx.kind, cx.kind == :lease && lease(cx), cx.kind == :request && request(cx)} do
        {:lease, nil, _} -> {:error, :Unknown}
        {:lease, l, _} -> l.lease
        {:request, _, nil} -> {:error, :Unknown}
        {:request, _, %{content: {:agent, _, lease}}} -> lease
        {:request, _, _} -> :ok
        _ -> {:error, :Unknown}
      end

    case asked do
      n when is_integer(n) -> ok_if(n > 0 and n <= @max_lease, :BadLease)
      other -> other
    end
  end

  # An agent request names a labelled agent: a labelled caller's in exactly its own label set
  # (R37); an unlabelled agent is StartAgent's.
  def agent_own_set(cx) do
    case request(cx) do
      nil ->
        {:error, :Unknown}

      %{content: {:agent, l, _}} ->
        own = labels(cx) == [] or Steward.labels(l) == labels(cx)
        ok_if(l != [] and own, :NotOwner)

      _ ->
        :ok
    end
  end

  # R42: a declassification from a session with exactly the item's labels, a push from an
  # unlabelled one; each crosses a label.
  def exact_labels(cx) do
    case request(cx) do
      nil -> {:error, :Unknown}
      %{content: {:declassify, l, _}} -> ok_if(Steward.labels(l) == labels(cx) and labels(cx) != [], :NotOwner)
      %{content: {:push, _, t, _}} -> ok_if(labels(cx) == [] and t != [], :NotOwner)
      _ -> :ok
    end
  end

  # Printable ASCII and newlines.
  def printable?(b), do: for(<<c <- b>>, reduce: true, do: (acc -> acc and (c in 0x20..0x7E or c == ?\n)))

  # R42: a declassified item is at most 256 bytes of printable text.
  def item_fits(cx) do
    case request(cx) do
      nil ->
        {:error, :Unknown}

      r ->
        b = r.snapshot || ""

        cond do
          byte_size(b) > @declassify_max -> {:error, :TooBig}
          true -> ok_if(printable?(b), :NotPrintable)
        end
    end
  end

  # R38's binding: answered only on the channel that rendered it last, naming its hash.
  def rendered_here(cx) do
    case request(cx) do
      nil -> {:error, :Unknown}
      r -> ok_if(r.channel != nil and r.channel == cx.channel, :NotRendered)
    end
  end

  def hash_matches(cx) do
    case {request(cx), cx.event.kind} do
      {nil, _} -> {:error, :Unknown}
      {r, {:approve, _, _, hash}} -> ok_if(hash == r.hash, :HashMismatch)
      _ -> {:error, :HashMismatch}
    end
  end

  # R39: a lease is ended only from an unlabelled session of its sponsor.
  def sponsor_session(cx) do
    case cx.caller do
      nil ->
        {:error, :NotSponsor}

      by ->
        {account, by_labels} = by.domain
        ok_if(by.kind == :session and account == elem(cx.domain, 0) and by_labels == [], :NotSponsor)
    end
  end

  # The kind guards: they read an object's kind, and carry no rule.
  def granted(cx), do: ok_if(match?(%{granted: true}, lease(cx)), :Unknown)
  def grants_lease(cx), do: ok_if(match?(%{content: {:agent, _, _}}, request(cx)), :Unknown)
  def declassifies(cx), do: ok_if(match?(%{content: {:declassify, _, _}}, request(cx)), :Unknown)
  def pushes(cx), do: ok_if(match?(%{content: {:push, _, _, _}}, request(cx)), :Unknown)
  def reading(cx), do: ok_if(match?(%{kind: :read}, crossing(cx)), :Unknown)
  def copying(cx), do: ok_if(match?(%{kind: :copy_out}, crossing(cx)), :Unknown)
end
