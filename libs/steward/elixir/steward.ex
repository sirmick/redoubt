# The steward's policy core, its Elixir reference (docs/servers/steward.md, "Two embedders and a
# reference"): a differential oracle for the Rust core (libs/steward), never authoritative and
# never on the box. The rows come from the generator (gen/*.ex); the routing, the row
# interpreter, the guards and the effects are written here by hand, from the page and the tables.
#
# Values: a domain is {account, labels} with labels sorted and without repeats; an object is
# {domain, kind, id}; a token is {object, slot}. An event is %{now, random, reply, kind}, its
# kind a tuple named for the page's event ({:login, principal, labels, context, key, from}, ...).
defmodule Redoubt.Steward do
  alias Redoubt.Steward.{Effects, Guards}
  alias Redoubt.Steward.Gen

  @max_labels 8

  defstruct fixed: nil,
            routes: %{},
            ids: %{},
            channels: %{},
            attachments: %{},
            order: [],
            domains: %{},
            used: %{},
            exited: false,
            rows: MapSet.new(),
            broken: nil

  defmodule DomainState do
    @moduledoc false
    defstruct sessions: %{},
              leases: %{},
              requests: %{},
              crossings: %{},
              blame: %{state: :open, times: [], until: 0},
              sessions_started: 0,
              agents_started: 0
  end

  @machines %{
    session: Gen.Session,
    lease: Gen.Lease,
    request: Gen.Request,
    crossing: Gen.Crossing,
    blame: Gen.Blame,
    channel: Gen.ApprovalChannel
  }

  # The table each kind's machine is named by in the generator's row list.
  @names %{
    session: "session",
    lease: "lease",
    request: "request",
    crossing: "crossing",
    blame: "blame",
    channel: "approval_channel"
  }

  def max_labels, do: @max_labels

  # A label set: sorted, without repeats, at most @max_labels long; nil if too long.
  def labels(l) do
    s = l |> Enum.uniq() |> Enum.sort()
    if length(s) <= @max_labels, do: s, else: nil
  end

  def includes?(set, other), do: Enum.all?(other, &(&1 in set))

  # ---- Boot ------------------------------------------------------------------------------

  # The manifest checked (accounts non-zero and distinct, names distinct, label sets valid and
  # distinct, no key in two roles or held by keyd), and what boot carves; nil if refused.
  # `broken` names one guard the run replaces with one that always holds (the negative run).
  def boot(m, broken \\ nil) do
    with {:ok, principals} <- principals(m.principals, [], MapSet.new(), MapSet.new()),
         logins = Enum.flat_map(m.principals, & &1.login_keys) |> MapSet.new(),
         approvals = Enum.flat_map(m.principals, & &1.approval_keys) |> MapSet.new(),
         keyd = m.keyd_keys |> Enum.uniq() |> Enum.sort(),
         true <- MapSet.disjoint?(logins, approvals),
         true <- Enum.all?(keyd, &(&1 not in logins and &1 not in approvals)) do
      fixed = %{principals: principals, keyd: keyd, servers: m.servers, sizes: m.sizes}
      order = Enum.flat_map(principals, & &1.domains)
      store = %__MODULE__{fixed: fixed, order: order, broken: broken}
      # Each domain's blame starts at the blame table's Boot row.
      store =
        Enum.reduce(order, store, fn d, s ->
          s = put_in(s.domains[d], %DomainState{})
          cx = cx(s, new_out(), d, :blame, 0, %{now: 0, random: [], reply: 0, kind: :boot})
          {next, cx} = interpret(cx, nil, :boot)
          {:to, state} = next
          put_in(cx.store.domains[d].blame.state, state).store
        end)

      {store, carves(fixed)}
    else
      _ -> nil
    end
  end

  defp principals([], acc, _, _), do: {:ok, Enum.reverse(acc)}

  defp principals([p | rest], acc, accounts, names) do
    domains = Enum.map(p.label_sets, &{p.account, labels(&1)})
    owned = labels(p.owned)

    cond do
      p.account == 0 or p.account in accounts or p.name in names -> :refused
      Enum.any?(domains, fn {_, l} -> l == nil end) or owned == nil -> :refused
      length(Enum.uniq(domains)) != length(domains) -> :refused
      true ->
        q = %{
          name: p.name,
          account: p.account,
          login_keys: p.login_keys,
          approval_keys: p.approval_keys,
          owned: owned,
          domains: domains,
          top: p.top
        }

        principals(rest, [q | acc], MapSet.put(accounts, p.account), MapSet.put(names, p.name))
    end
  end

  # An equal share of the top budget per label set, less each sub-budget's own object.
  defp carves(fixed) do
    cost = fixed.sizes.budget_cost

    Enum.map(fixed.principals, fn p ->
      n = max(length(p.domains), 1)
      {pages, processes, weight} = p.top
      share = {max(div(pages, n) - cost, 0), div(processes, n), div(weight, n)}
      {p.account, p.top, Enum.map(p.domains, &{&1, share})}
    end)
  end

  # ---- Deciding --------------------------------------------------------------------------

  def new_out, do: %{outputs: [], batches: [], raised: :queue.new(), steps: [], drawn: [], exit: false}

  # One event: through the machine of the object it is about, then the events machines raise
  # for each other, in order. After an event the embedder's guarantee excludes, the steward has
  # exited and decides nothing more. Returns {store, effects}.
  def decide(%__MODULE__{exited: true} = store, _event), do: {store, exited()}

  def decide(store, event) do
    {store, out} = external(store, event, new_out())
    {store, out} = drain(store, event, out)

    if out.exit do
      {%{store | exited: true}, exited()}
    else
      {store, %{outputs: Enum.reverse(out.outputs), batches: Enum.reverse(out.batches), exit: false}}
    end
  end

  defp exited, do: %{outputs: [], batches: [], exit: true}

  defp drain(store, event, out) do
    case :queue.out(out.raised) do
      {:empty, _} ->
        {store, out}

      {{:value, raised}, q} ->
        if out.exit do
          {store, out}
        else
          {store, out} = internal(store, event, raised, %{out | raised: q})
          drain(store, event, out)
        end
    end
  end

  # A transition's context: the store, the one domain it is about and its object, the event,
  # and what it emits.
  defp cx(store, out, domain, kind, id, event, call \\ %{}) do
    Map.merge(
      %{
        store: store,
        out: out,
        domain: domain,
        kind: kind,
        id: id,
        event: event,
        caller: nil,
        channel: nil,
        grant: nil,
        result: nil,
        reply: event.reply,
        reason: nil
      },
      call
    )
  end

  # The first row for (from, event) whose guards hold: its effects run in order, and it says
  # where the object goes. A guard `g` must hold; `{:not, g}` must fail, and its reason is the
  # one the row's refusal gives.
  def interpret(cx, from, event) do
    case Map.fetch!(@machines, cx.kind).rows(from, event) do
      :no_row -> {:no_row, cx}
      rows -> take(rows, cx)
    end
  end

  defp take([{line, guards, effects, to} | rest], cx) do
    case hold(guards, cx) do
      {false, cx} ->
        take(rest, cx)

      {true, cx} ->
        cx = put_in(cx.store.rows, MapSet.put(cx.store.rows, {Map.fetch!(@names, cx.kind), line}))

        case effects do
          :unreachable -> {:unreachable, cx}
          list -> {to, Enum.reduce(list, cx, &apply(Effects, &1, [&2]))}
        end
    end
  end

  defp hold([], cx), do: {true, cx}

  defp hold([{:not, g} | rest], cx) do
    case guard(g, cx) do
      :ok -> {false, cx}
      {:error, why} -> hold(rest, %{cx | reason: why})
    end
  end

  defp hold([g | rest], cx) do
    case guard(g, cx) do
      :ok -> hold(rest, cx)
      {:error, _} -> {false, cx}
    end
  end

  defp guard(g, %{store: %{broken: g}}), do: :ok
  defp guard(g, cx), do: apply(Guards, g, [cx])

  # One transition of the object `id` of `kind` in `domain`. A new object is already in the
  # store; `created` says so. Applies where the row sends the object, and makes the steps the
  # row emitted its batch.
  defp run(store, out, event, domain, kind, id, e, call \\ %{}) do
    case store.domains[domain] do
      nil -> {:no_row, store, out}
      st -> run_in(store, out, event, domain, st, kind, id, e, call)
    end
  end

  defp run_in(store, out, event, domain, st, kind, id, e, call) do
    {created, call} = Map.pop(call, :created, false)
    stored = object(st, kind, id)
    from = if stored == nil or created, do: nil, else: stored.state
    # A batch's outcome answers the call that started the batch.
    reply = if Map.get(call, :result) != nil, do: (stored && Map.get(stored, :reply)) || 0, else: event.reply
    cx = cx(store, out, domain, kind, id, event, Map.put(call, :reply, reply))
    {next, cx} = interpret(cx, from, e)
    store = cx.store
    out = cx.out
    finals = Map.fetch!(@machines, kind).finals()

    {store, out} =
      case next do
        {:to, s} ->
          if s in finals do
            {remove(store, domain, kind, id) |> forget_id(id), out}
          else
            {set_state(store, domain, kind, id, s), out}
          end

        n when n in [:nothing, :no_row] and created ->
          {remove(store, domain, kind, id) |> forget_id(id), out}

        :unreachable ->
          {store, %{out | exit: true}}

        _ ->
          {store, out}
      end

    out =
      case out.steps do
        [] -> out
        steps -> %{out | steps: [], batches: [{{domain, kind, id}, steps} | out.batches]}
      end

    {next, store, out}
  end

  # The object is gone: its id, and any attachment naming it, name nothing from here.
  defp forget_id(store, id) do
    attachments = store.attachments |> Enum.reject(fn {_, {_, s}} -> s == id end) |> Map.new()
    %{store | ids: Map.delete(store.ids, id), attachments: attachments}
  end

  defp field(:session), do: :sessions
  defp field(:lease), do: :leases
  defp field(:request), do: :requests
  defp field(:crossing), do: :crossings

  defp object(st, :blame, _), do: st.blame
  defp object(st, kind, id), do: Map.get(Map.fetch!(st, field(kind)), id)

  defp set_state(store, d, :blame, _, s), do: put_in(store.domains[d].blame.state, s)

  defp set_state(store, d, kind, id, s) do
    update_in(store.domains[d], fn st ->
      Map.update!(st, field(kind), fn m ->
        if Map.has_key?(m, id), do: Map.update!(m, id, &%{&1 | state: s}), else: m
      end)
    end)
  end

  defp remove(store, _, :blame, _), do: store

  defp remove(store, d, kind, id),
    do: update_in(store.domains[d], &Map.update!(&1, field(kind), fn m -> Map.delete(m, id) end))

  defp has?(_st, :blame, id), do: id == 0
  defp has?(st, kind, id), do: Map.has_key?(Map.fetch!(st, field(kind)), id)

  # A request or crossing id leaves `used` when its object goes.
  defp release(store, d, kind, id) do
    case store.domains[d] do
      %DomainState{} = st -> if has?(st, kind, id), do: store, else: drop_used(store, id)
      nil -> drop_used(store, id)
    end
  end

  defp drop_used(store, id), do: %{store | used: Map.delete(store.used, id)}

  # Puts a new object in its domain, and its id in the index, before its first transition.
  defp insert(store, d, kind, id, obj) do
    case store.domains[d] do
      nil ->
        {false, store}

      _ ->
        store = update_in(store.domains[d], &Map.update!(&1, field(kind), fn m -> Map.put(m, id, obj) end))

        store =
          if kind in [:session, :lease],
            do: %{store | ids: Map.put(store.ids, id, {d, kind})},
            else: %{store | used: Map.put(store.used, id, d)}

        {true, store}
    end
  end

  # A fresh id from the event's words: never 0 and naming nothing yet. When the words run out,
  # the embedder broke its guarantee, and the steward exits.
  def fresh(store, event, out) do
    w =
      Enum.find(event.random, fn w ->
        w != 0 and not Map.has_key?(store.ids, w) and not Map.has_key?(store.routes, w) and
          not Map.has_key?(store.channels, w) and not Map.has_key?(store.used, w) and
          not Map.has_key?(store.attachments, w) and w not in out.drawn
      end)

    case w do
      nil -> {0, %{out | exit: true}}
      w -> {w, %{out | drawn: out.drawn ++ [w]}}
    end
  end

  defp reply(out, %{reply: 0}, _), do: out
  defp reply(out, event, answer), do: %{out | outputs: [{:reply, event.reply, answer} | out.outputs]}

  defp unknown(store, event, out), do: {store, reply(out, event, {:refused, :Unknown})}

  defp answered({:no_row, store, out}, event), do: unknown(store, event, out)
  defp answered({_, store, out}, _), do: {store, out}

  defp done({_, store, out}), do: {store, out}

  # A principal's, a label's or a context's name: 1 to 64 bytes of [a-z0-9_-], a letter first.
  def name?(<<c, _::binary>> = s) when c in ?a..?z and byte_size(s) <= 64,
    do: for(<<c <- s>>, reduce: true, do: (ok -> ok and (c in ?a..?z or c in ?0..?9 or c in [?_, ?-])))

  def name?(_), do: false

  def principal(fixed, name), do: Enum.find_index(fixed.principals, &(&1.name == name))
  def by_account(fixed, account), do: Enum.find_index(fixed.principals, &(&1.account == account))

  # The domain of a label set of `account`, if the manifest names it.
  defp find(store, account, l) do
    case labels(l) do
      nil -> nil
      set -> Enum.find(store.order, &(&1 == {account, set}))
    end
  end

  defp route(store, badge) do
    case store.routes[badge] do
      nil -> nil
      {d, kind, id} -> %{domain: d, kind: kind, id: id}
    end
  end

  # ---- Events from outside the core ------------------------------------------------------

  # Before the key is checked, every refusal is the bad key's: an unknown principal, a label set
  # the manifest does not give it and a context that is not a name.
  defp external(store, %{kind: {:login, name, l, context, key, _from}} = e, out) do
    with p when p != nil <- principal(store.fixed, name),
         account = Enum.at(store.fixed.principals, p).account,
         d when d != nil <- find(store, account, l),
         true <- context == "" or name?(context) do
      {_, store, out} = run(store, out, e, d, :blame, 0, :login)
      {id, out} = fresh(store, e, out)
      {badge, out} = fresh(store, e, out)

      s = %{
        id: id,
        state: :starting,
        principal: p,
        key: key,
        context: context,
        badge: badge,
        number: 0,
        reply: e.reply,
        attachment: id,
        from: ""
      }

      {_, store} = insert(store, d, :session, id, s)
      done(run(store, out, e, d, :session, id, :login, %{created: true}))
    else
      _ -> {store, reply(out, e, {:refused, :BadKey})}
    end
  end

  # The console principal's session on the UART: no key, in the principal's unlabelled domain,
  # otherwise a login's.
  defp external(store, %{kind: {:console, name}} = e, out) do
    with p when p != nil <- principal(store.fixed, name),
         account = Enum.at(store.fixed.principals, p).account,
         d when d != nil <- find(store, account, []) do
      {_, store, out} = run(store, out, e, d, :blame, 0, :login)
      {id, out} = fresh(store, e, out)
      {badge, out} = fresh(store, e, out)

      s = %{
        id: id,
        state: :starting,
        principal: p,
        key: 0,
        context: nil,
        badge: badge,
        number: 0,
        reply: e.reply,
        attachment: id,
        from: ""
      }

      {_, store} = insert(store, d, :session, id, s)
      done(run(store, out, e, d, :session, id, :console, %{created: true}))
    else
      nil -> unknown(store, e, out)
    end
  end

  # A channel is named by its attachment; one no context holds now names nothing.
  defp external(store, %{kind: {:channel_closed, attachment}} = e, out) do
    case store.attachments[attachment] do
      {d, id} -> answered(run(store, out, e, d, :session, id, :channel_closed), e)
      _ -> unknown(store, e, out)
    end
  end

  # Every channel went with `sshd`: each attached context is detached, in attachment order.
  defp external(store, %{kind: :sshd_gone} = e, out) do
    store.attachments
    |> Enum.sort()
    |> Enum.reduce({store, out}, fn {_, {d, id}}, {store, out} ->
      {_, store, out} = run(store, out, e, d, :session, id, :detach)
      {store, out}
    end)
  end

  defp external(store, %{kind: {:approval_opened, _, _, _}} = e, out), do: channel(store, e, out)
  defp external(store, %{kind: {:approval_closed, _}} = e, out), do: channel(store, e, out)

  defp external(store, %{kind: {:start_agent, badge, lease}} = e, out) do
    with by when by != nil <- route(store, badge),
         p when p != nil <- by_account(store.fixed, elem(by.domain, 0)) do
      {id, out} = fresh(store, e, out)
      {own, out} = fresh(store, e, out)

      l = %{
        id: id,
        state: :starting,
        principal: p,
        badge: own,
        number: 0,
        lease: lease,
        deadline: 0,
        parent: if(by.kind == :lease, do: by.id, else: nil),
        granted: false,
        reply: e.reply
      }

      {_, store} = insert(store, by.domain, :lease, id, l)
      done(run(store, out, e, by.domain, :lease, id, :start_agent, %{caller: by, created: true}))
    else
      nil -> unknown(store, e, out)
    end
  end

  defp external(store, %{kind: {:submit, badge, content, reason}} = e, out) do
    with by when by != nil <- route(store, badge),
         p when p != nil <- by_account(store.fixed, elem(by.domain, 0)) do
      # The domain its records are read under: a labelled agent's or a push's target, which must
      # be one of the principal's.
      audit =
        case content do
          {:agent, l, _} -> find(store, elem(by.domain, 0), l)
          {:push, _, l, _} -> find(store, elem(by.domain, 0), l)
          _ -> by.domain
        end

      if audit == nil do
        {store, reply(out, e, {:refused, :NotOwner})}
      else
        {id, out} = fresh(store, e, out)

        r = %{
          id: id,
          state: :frozen,
          by: {by.kind, by.id},
          principal: p,
          content: content,
          reason: reason,
          snapshot: nil,
          reader: 0,
          hash: <<0::256>>,
          audit: audit,
          channel: nil,
          reply: e.reply
        }

        {_, store} = insert(store, by.domain, :request, id, r)
        {_, store, out} = run(store, out, e, by.domain, :request, id, :submit, %{caller: by, created: true})
        {release(store, by.domain, :request, id), out}
      end
    else
      nil -> unknown(store, e, out)
    end
  end

  defp external(store, %{kind: {:end_lease, _, _}} = e, out), do: end_lease(store, e, out)

  defp external(store, %{kind: {:end_session, badge}} = e, out) do
    case route(store, badge) do
      %{kind: :session} = by -> done(run(store, out, e, by.domain, :session, by.id, :end_session, %{caller: by}))
      _ -> unknown(store, e, out)
    end
  end

  defp external(store, %{kind: {k, _}} = e, out) when k in [:pending], do: approval(store, e, out)
  defp external(store, %{kind: {k, _, _, _}} = e, out) when k in [:approve], do: approval(store, e, out)
  defp external(store, %{kind: {k, _, _}} = e, out) when k in [:deny], do: approval(store, e, out)

  defp external(store, %{kind: {:blame, account, l}} = e, out) do
    # A crash with no current call blames nobody.
    case find(store, account, l) do
      nil -> {store, out}
      d -> done(run(store, out, e, d, :blame, 0, :blame))
    end
  end

  defp external(store, %{kind: {:exited, {d, kind, id}}} = e, out) do
    case kind do
      k when k in [:session, :lease] ->
        done(run(store, out, e, d, k, id, :exited))

      :crossing ->
        {_, store, out} = run(store, out, e, d, :crossing, id, :exited)
        {release(store, d, :crossing, id), out}

      _ ->
        {store, out}
    end
  end

  defp external(store, %{kind: {:done, {d, kind, id}, result}} = e, out) do
    ev = if match?({:ok, _}, result), do: :done, else: :failed
    call = %{result: result}

    case kind do
      k when k in [:session, :lease] ->
        done(run(store, out, e, d, k, id, ev, call))

      :request ->
        # A push's source, read by the steward: its snapshot.
        store =
          case result do
            {:ok, list} ->
              bytes = Enum.find_value(list, fn p -> match?({:bytes, _}, p) && elem(p, 1) end)

              if store.domains[d] && store.domains[d].requests[id],
                do: put_in(store.domains[d].requests[id].snapshot, bytes),
                else: store

            _ ->
              store
          end

        {_, store, out} = run(store, out, e, d, :request, id, ev, call)
        {release(store, d, :request, id), out}

      :crossing ->
        {_, store, out} = run(store, out, e, d, :crossing, id, ev, call)
        {release(store, d, :crossing, id), out}

      _ ->
        {store, out}
    end
  end

  # ---- Events machines raise for each other ----------------------------------------------

  defp internal(store, e, {:granted, d, id, p, lease}, out) do
    {badge, out} = fresh(store, e, out)

    l = %{
      id: id,
      state: :starting,
      principal: p,
      badge: badge,
      number: 0,
      lease: lease,
      deadline: 0,
      parent: nil,
      granted: true,
      reply: 0
    }

    case insert(store, d, :lease, id, l) do
      {true, store} -> done(run(store, out, e, d, :lease, id, :granted, %{created: true}))
      {false, store} -> {store, out}
    end
  end

  defp internal(store, e, {:open, d, crossing}, out), do: cross(store, e, d, crossing, out)

  defp internal(store, e, {:snapshot, {d, :request, id}, result}, out) do
    if store.domains[d] && store.domains[d].requests[id] do
      store =
        case result do
          {bytes, reader} ->
            update_in(store.domains[d].requests[id], &%{&1 | snapshot: bytes, reader: reader})

          nil ->
            store
        end

      # The request's batch is the crossing's: its outcome answers the submission.
      {ev, outcome} = if result, do: {:done, {:ok, []}}, else: {:failed, {:error, 0, 0}}
      {_, store, out} = run(store, out, e, d, :request, id, ev, %{result: outcome})
      {release(store, d, :request, id), out}
    else
      {store, out}
    end
  end

  defp internal(store, e, {:locked_out, {d, kind, id}}, out) when kind in [:session, :lease],
    do: done(run(store, out, e, d, kind, id, :locked_out))

  defp internal(store, e, {:attach, {d, :session, id}}, out), do: done(run(store, out, e, d, :session, id, :attach))

  defp internal(store, e, {:session_ended, {d, :request, id}}, out) do
    {_, store, out} = run(store, out, e, d, :request, id, :session_ended)
    {release(store, d, :request, id), out}
  end

  # ---- Approval channels: outside every domain -------------------------------------------

  defp channel(store, %{kind: {:approval_opened, id, name, key}} = e, out) do
    case principal(store.fixed, name) do
      nil ->
        unknown(store, e, out)

      p ->
        cond do
          # The same channel opened twice: its own row refuses it.
          Map.has_key?(store.channels, id) ->
            run_channel(store, out, e, id, store.channels[id].principal, :approval_opened, false)

          id == 0 or Map.has_key?(store.ids, id) or Map.has_key?(store.routes, id) ->
            unknown(store, e, out)

          true ->
            c = %{id: id, state: :open, principal: p, key: key}
            store = %{store | channels: Map.put(store.channels, id, c)}
            run_channel(store, out, e, id, p, :approval_opened, true)
        end
    end
  end

  defp channel(store, %{kind: {:approval_closed, id}} = e, out) do
    case store.channels[id] do
      nil -> unknown(store, e, out)
      c -> run_channel(store, out, e, id, c.principal, :approval_closed, false)
    end
  end

  defp run_channel(store, out, e, id, p, ev, created) do
    d = {Enum.at(store.fixed.principals, p).account, []}
    from = if created, do: nil, else: store.channels[id].state
    cx = cx(store, out, d, :channel, id, e, %{channel: id})
    {next, cx} = interpret(cx, from, ev)
    store = cx.store
    out = cx.out

    case next do
      {:to, :closed} ->
        {unbind(%{store | channels: Map.delete(store.channels, id)}, id), out}

      {:to, s} ->
        {put_in(store.channels[id].state, s), out}

      n when n in [:nothing, :no_row] and created ->
        {%{store | channels: Map.delete(store.channels, id)}, out}

      :unreachable ->
        {store, %{out | exit: true}}

      _ ->
        {store, out}
    end
  end

  # A closed channel's renders: each request it rendered last needs a render again, so a
  # channel opened later under its id answers none of them (R38's binding).
  defp unbind(store, channel) do
    domains =
      Map.new(store.domains, fn {d, st} ->
        requests =
          Map.new(st.requests, fn {id, r} ->
            {id, if(r.channel == channel, do: %{r | channel: nil}, else: r)}
          end)

        {d, %{st | requests: requests}}
      end)

    %{store | domains: domains}
  end

  # ---- The three edges that cross domains (R34) -------------------------------------------

  # Whether an approval channel of principal `p` reaches the requests of domain `d`: its own
  # account's, whose every label it owns (R38).
  def reaches(store, p, {account, l}) do
    pr = Enum.at(store.fixed.principals, p)
    account == pr.account and includes?(pr.owned, l)
  end

  # The request and approval path.
  defp approval(store, e, out) do
    ch = elem(e.kind, 1)

    case store.channels[ch] do
      nil -> unknown(store, e, out)
      c -> approve(store, e, out, ch, c.principal)
    end
  end

  defp approve(store, %{kind: {:pending, _}} = e, out, ch, p) do
    # Every pending request of the domains it reaches is offered to the channel.
    Enum.filter(store.order, &reaches(store, p, &1))
    |> Enum.reduce({store, out}, fn d, {store, out} ->
      ids = store.domains[d].requests |> Map.keys() |> Enum.sort()

      Enum.reduce(ids, {store, out}, fn id, {store, out} ->
        done(run(store, out, e, d, :request, id, :pending, %{channel: ch}))
      end)
    end)
  end

  defp approve(store, e, out, ch, p) do
    {request, ev} =
      case e.kind do
        {:approve, _, r, _} -> {r, :approve}
        {:deny, _, r} -> {r, :deny}
      end

    with d when d != nil <- store.used[request],
         true <- reaches(store, p, d),
         r when r != nil <- store.domains[d].requests[request] do
      # The domain an approved labelled agent would start in, when it is another.
      grant = if match?({:agent, _, _}, r.content) and ev == :approve and r.audit != d, do: r.audit

      result =
        case grant do
          nil ->
            run(store, out, e, d, :request, request, ev, %{channel: ch})

          g ->
            case store.domains[g] do
              nil -> {:no_row, store, out}
              gst -> run(store, out, e, d, :request, request, ev, %{channel: ch, grant: gst})
            end
        end

      {next, store, out} = result
      answered({next, release(store, d, :request, request), out}, e)
    else
      _ -> unknown(store, e, out)
    end
  end

  # Lease supervision: an unlabelled session of the sponsor ends a lease, in the lease's domain.
  defp end_lease(store, %{kind: {:end_lease, badge, lease}} = e, out) do
    with by when by != nil <- route(store, badge),
         {d, :lease} <- store.ids[lease],
         true <- elem(d, 0) == elem(by.domain, 0) do
      answered(run(store, out, e, d, :lease, lease, :end_lease, %{caller: by}), e)
    else
      _ -> unknown(store, e, out)
    end
  end

  # The crossing: a request opens one in the labelled side's domain.
  defp cross(store, e, d, crossing, out) do
    case store.domains[d] do
      nil ->
        {store, out}

      _ ->
        id = crossing.id
        store = put_in(store.domains[d].crossings[id], crossing)
        store = %{store | used: Map.put(store.used, id, d)}
        {_, store, out} = run(store, out, e, d, :crossing, id, :open, %{created: true})
        {release(store, d, :crossing, id), out}
    end
  end
end
