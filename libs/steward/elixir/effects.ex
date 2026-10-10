# The effects the tables name (docs/servers/steward.md, "Guards and effects"), for the Elixir
# reference. Each takes the transition's context and returns it: it changes the object, adds a
# step to the object's batch, or emits outputs and raised events.
defmodule Redoubt.Steward.Effects do
  alias Redoubt.Steward
  alias Redoubt.Steward.{Guards, Render}

  # The slots of what a batch makes: a session's or lease's budget, scope and process, then its
  # connections (the steward's, then one per shared server); a read's bytes.
  @budget 0
  @scope 1
  @process 2
  @connections 3
  # A context's console relay, and the channel console it holds while attached.
  @relay 200
  @console 201
  @read 1
  @crossing_life 1_000_000
  @u64 0xFFFF_FFFF_FFFF_FFFF

  defp add(a, b), do: min(a + b, @u64)

  defp object(cx), do: {cx.domain, cx.kind, cx.id}
  defp token(cx, slot), do: {object(cx), slot}
  defp step(cx, s), do: put_in(cx.out.steps, cx.out.steps ++ [s])
  defp output(cx, o), do: put_in(cx.out.outputs, [o | cx.out.outputs])
  defp raise_(cx, r), do: put_in(cx.out.raised, :queue.in(r, cx.out.raised))

  defp reply(%{reply: 0} = cx, _), do: cx
  defp reply(cx, answer), do: output(cx, {:reply, cx.reply, answer})

  defp audit(cx, record), do: audit_in(cx, cx.domain, record)
  defp audit_in(cx, d, record), do: output(cx, {:audit, d, cx.event.now, record})

  defp update(cx, field, f) do
    update_in(cx.store.domains[cx.domain], fn st ->
      Map.update!(st, field, fn m -> if Map.has_key?(m, cx.id), do: Map.update!(m, cx.id, f), else: m end)
    end)
  end

  defp fresh(cx) do
    {w, out} = Steward.fresh(cx.store, cx.event, cx.out)
    {w, %{cx | out: out}}
  end

  # The row's refusal: the failed guard's reason, or a failed step's.
  def refuse(cx) do
    why = cx.reason || if(match?({:error, _, _}, cx.result), do: :Failed, else: :Unknown)
    reply(cx, {:refused, why})
  end

  def reply_ok(cx), do: reply(cx, :ok)
  def forget(cx), do: output(cx, {:forget, object(cx)})

  # Sessions and leases.

  def carve_session(cx) do
    sizes = cx.store.fixed.sizes
    step(cx, {:create_budget, token(cx, @budget), {:sub, cx.domain}, sizes.session, elem(cx.domain, 1), nil})
  end

  # R39: from the domain's sub-budget for its lease; a sub-agent inside its agent's budget, ending
  # no later.
  def carve_lease(cx) do
    case Guards.lease(cx) do
      nil ->
        cx

      l ->
        sizes = cx.store.fixed.sizes
        deadline = add(cx.event.now, l.lease)
        agent = l.parent && Guards.state(cx).leases[l.parent]

        {parent, limits, deadline} =
          case agent do
            nil -> {{:sub, cx.domain}, sizes.agent, deadline}
            a -> {{:budget, {{cx.domain, :lease, a.id}, @budget}}, sizes.sub_agent, min(deadline, a.deadline)}
          end

        cx = update(cx, :leases, &%{&1 | deadline: deadline})
        step(cx, {:create_budget, token(cx, @budget), parent, limits, elem(cx.domain, 1), deadline})
    end
  end

  def create_scope(cx), do: step(cx, {:create_scope, token(cx, @scope), token(cx, @budget)})

  defp badge(cx) do
    o =
      case cx.kind do
        :session -> Guards.session(cx)
        :lease -> Guards.lease(cx)
        _ -> nil
      end

    (o && o.badge) || 0
  end

  # The connections, each narrowed to the scope this batch made: the steward's, routed by the
  # object's badge, and a fresh-badged one per shared server.
  def connect(cx) do
    scope = cx.out.steps |> Enum.reverse() |> Enum.find_value(&match?({:create_scope, _, _}, &1) && elem(&1, 1))

    if scope do
      cx = step(cx, {:connect, token(cx, @connections), scope, :steward, badge(cx)})

      Enum.reduce(0..(cx.store.fixed.servers - 1)//1, cx, fn i, cx ->
        {b, cx} = fresh(cx)
        step(cx, {:connect, token(cx, @connections + 1 + i), scope, {:shared, i}, b})
      end)
    else
      cx
    end
  end

  def launch(cx) do
    conns = for i <- 0..cx.store.fixed.servers, do: token(cx, @connections + i)
    step(cx, {:launch, token(cx, @process), token(cx, @budget), conns})
  end

  # A context's console relay, in the session's budget, before the process it serves; the
  # console's session is no context and has none.
  def launch_relay(cx) do
    case Guards.session(cx) do
      %{context: c} when c != nil -> step(cx, {:launch_relay, token(cx, @relay), token(cx, @budget)})
      _ -> cx
    end
  end

  # The login's address as a note shows it: `a.b.c.d:port`, or `an unknown address`.
  defp login_from(%{event: %{kind: {:login, _, _, _, _, from}}}) do
    ok =
      from != "" and byte_size(from) <= 21 and
        for(<<b <- from>>, reduce: true, do: (ok -> ok and (b in ?0..?9 or b in [?., ?:])))

    if ok, do: from, else: "an unknown address"
  end

  defp login_from(_), do: ""

  defp context_name(cx) do
    case Guards.session(cx) do
      %{context: c} when c not in [nil, ""] -> c
      _ -> "default"
    end
  end

  # R80: the context's console goes to the login's channel, with a note saying how.
  def attach_relay(%{event: %{kind: {:login, _, _, _, key, _}}} = cx) do
    {from, name, reply} = {login_from(cx), context_name(cx), cx.reply}

    {fresh, cx} =
      case Guards.session(cx) do
        %{attachment: 0} -> fresh(cx)
        _ -> {0, cx}
      end

    case Guards.session(cx) do
      nil ->
        cx

      s ->
        note =
          cond do
            s.number == 0 -> ""
            s.from == "" -> "[context #{name}: reattached]\r\n"
            true -> "[context #{name}: reattached; taken over from #{s.from}]\r\n"
          end

        attachment = if s.attachment == 0, do: fresh, else: s.attachment
        now = cx.event.now

        cx =
          update(cx, :sessions, &%{&1 | attachment: attachment, from: from, key: key, reply: reply, since: now, idle: nil})
        cx = put_in(cx.store.attachments[attachment], {cx.domain, cx.id})
        step(cx, {:attach, token(cx, @relay), token(cx, @console), note})
    end
  end

  def attach_relay(cx), do: cx

  # R80: the attached channel lets it go, told why if a login took it over; its id names
  # nothing from here.
  def detach_relay(cx), do: detach_with(cx, true)

  def detach_with(cx, forget) do
    now = div(cx.event.now, 1_000_000)

    note =
      case cx.event.kind do
        {:login, _, _, _, _, _} ->
          m = String.pad_leading(Integer.to_string(div(rem(now, 3600), 60)), 2, "0")
          "[context #{context_name(cx)} taken over from #{login_from(cx)} at up #{div(now, 3600)}h#{m}m]\r\n"

        _ ->
          ""
      end

    case Guards.session(cx) do
      nil ->
        cx

      s ->
        from = if note == "", do: "", else: s.from
        at = cx.event.now
        cx = update(cx, :sessions, &%{&1 | attachment: 0, from: from, since: at, idle: &1.idle || at})

        cx =
          if forget,
            do: put_in(cx.store.attachments, Map.delete(cx.store.attachments, s.attachment)),
            else: cx

        step(cx, {:detach, token(cx, @relay), token(cx, @console), note})
    end
  end

  # A login, authenticated, names a context whose session lives: that session takes the login.
  def take_over(cx) do
    case Guards.session(cx) do
      %{context: name} when name != nil ->
        live =
          Guards.state(cx).sessions
          |> Enum.sort()
          |> Enum.find(fn {id, o} -> id != cx.id and o.state != :ending and o.context == name end)

        case live do
          {id, _} -> raise_(cx, {:attach, {cx.domain, :session, id}})
          nil -> cx
        end

      _ ->
        cx
    end
  end

  def refuse_in_use(cx), do: reply(cx, {:refused, :InUse})

  def audit_attached(cx) do
    case Guards.session(cx) do
      nil -> cx
      s -> audit(cx, {:attached, s.id, s.key, s.from, s.state == :running})
    end
  end

  # A detached context past its idle bound ends: recorded, with how long it was idle (seconds).
  def audit_idle(cx) do
    case Guards.session(cx) do
      nil -> cx
      s -> audit(cx, {:idle_ended, s.id, div(cx.event.now - (s.idle || cx.event.now), 1_000_000)})
    end
  end

  def destroy_budget(cx), do: step(cx, {:destroy_budget, token(cx, @budget)})
  def destroy_partial(cx), do: destroy_budget(cx)

  # The object runs: its badge routes to it, and it gets its number in its domain (R37).
  def route(cx) do
    {count, field} = if cx.kind == :session, do: {:sessions_started, :sessions}, else: {:agents_started, :leases}
    cx = update_in(cx.store.domains[cx.domain], &Map.update!(&1, count, fn n -> n + 1 end))
    n = Map.fetch!(Guards.state(cx), count)

    case Map.fetch!(Guards.state(cx), field)[cx.id] do
      nil ->
        cx

      o ->
        cx = update(cx, field, &%{&1 | number: n})
        put_in(cx.store.routes[o.badge], {cx.domain, cx.kind, cx.id})
    end
  end

  def unroute(cx), do: put_in(cx.store.routes, Map.delete(cx.store.routes, badge(cx)))

  # A session's or an agent's end drops its requests.
  def drop_requests(cx) do
    Guards.state(cx).requests
    |> Map.values()
    |> Enum.filter(&(&1.by == {cx.kind, cx.id} and &1.state not in Steward.Gen.Request.finals()))
    |> Enum.map(& &1.id)
    |> Enum.sort()
    |> Enum.reduce(cx, &raise_(&2, {:session_ended, {cx.domain, :request, &1}}))
  end

  def audit_login(cx) do
    case Guards.session(cx) do
      nil -> cx
      s -> audit(cx, {:login, s.id, s.principal, s.key, s.context})
    end
  end

  def reply_login(cx) do
    case Guards.session(cx) do
      nil -> cx
      s -> reply(cx, {:session, s.attachment, "session-#{s.number}"})
    end
  end

  def audit_agent_started(cx) do
    case Guards.lease(cx) do
      nil -> cx
      l -> audit(cx, {:agent_started, l.id, l.principal, l.parent, l.deadline})
    end
  end

  def audit_start_failed(cx), do: audit(cx, {:start_failed, cx.id})

  def reply_agent(cx) do
    case Guards.lease(cx) do
      nil -> cx
      l -> reply(cx, {:lease, l.id, "agent-#{l.number}"})
    end
  end

  def audit_lease_ended(cx), do: audit(cx, {:lease_ended, cx.id})

  # The sponsor learns that the lease ended, not why: its unlabelled sessions are told.
  def notify_sponsor(cx) do
    sponsor = {elem(cx.domain, 0), []}

    cx.store.routes
    |> Enum.sort()
    |> Enum.filter(fn {_, {d, kind, _}} -> kind == :session and d == sponsor end)
    |> Enum.reduce(cx, fn {b, _}, cx -> output(cx, {:notice, {:session, b}, {:lease_ended, cx.id}}) end)
  end

  # Requests.

  defp new_crossing(cx, kind, item, bytes, source) do
    {id, cx} = fresh(cx)

    c = %{
      id: id,
      state: :open,
      kind: kind,
      request: object(cx),
      item: item,
      bytes: bytes,
      source: source,
      through: false
    }

    {c, cx}
  end

  # A declassification's read: a crossing of this, the labelled, domain.
  def open_read(cx) do
    case Guards.request(cx) do
      %{content: {:declassify, _, item}} ->
        {c, cx} = new_crossing(cx, :read, item, "", 0)
        raise_(cx, {:open, cx.domain, c})

      _ ->
        cx
    end
  end

  # A push's source, read by the steward itself: it is unlabelled.
  def read_source(cx) do
    case Guards.request(cx) do
      %{content: {:push, source, _, _}} -> step(cx, {:read, token(cx, @read), nil, [], source})
      _ -> cx
    end
  end

  def freeze(cx), do: update(cx, :requests, &%{&1 | hash: Render.binding(cx.domain, &1)})

  def audit_submitted(cx) do
    case Guards.request(cx) do
      nil -> cx
      r -> audit_in(cx, r.audit, {:submitted, r.id})
    end
  end

  def reply_request(cx), do: reply(cx, {:request, cx.id})

  # R38: an approval is waiting, told to the sessions whose labels include all the request's and
  # to the principal's approval channels.
  def notify(cx) do
    case Guards.request(cx) do
      nil ->
        cx

      r ->
        {account, l} = cx.domain

        sessions =
          for {b, {{a, dl}, _, _}} <- Enum.sort(cx.store.routes), a == account, Steward.includes?(dl, l),
              do: {:session, b}

        channels =
          for {id, c} <- Enum.sort(cx.store.channels), c.principal == r.principal, do: {:channel, id}

        Enum.reduce(sessions ++ channels, cx, &output(&2, {:notice, &1, :approval_waiting}))
    end
  end

  # R38's screen, on the channel that asked, which the request is now bound to.
  def render(cx) do
    case {Guards.request(cx), cx.channel} do
      {nil, _} ->
        cx

      {_, nil} ->
        cx

      {r, ch} ->
        screen = Render.screen(cx.store.fixed, cx.domain, Guards.state(cx), r)
        cx = update(cx, :requests, &%{&1 | channel: ch})
        output(cx, {:screen, ch, screen})
    end
  end

  defp channel_of(cx), do: cx.channel && cx.store.channels[cx.channel]

  def audit_approved(cx) do
    case {Guards.request(cx), channel_of(cx)} do
      {r, c} when r != nil and c != nil -> audit_in(cx, r.audit, {:approved, r.id, c.principal, c.key, r.hash})
      _ -> cx
    end
  end

  def audit_denied(cx) do
    case {Guards.request(cx), channel_of(cx)} do
      {r, c} when r != nil and c != nil -> audit_in(cx, r.audit, {:denied, r.id, c.principal})
      _ -> cx
    end
  end

  # An approved labelled agent: a lease in its own domain.
  def grant_lease(cx) do
    case Guards.request(cx) do
      %{content: {:agent, _, lease}} = r ->
        {id, cx} = fresh(cx)
        raise_(cx, {:granted, r.audit, id, r.principal, lease})

      _ ->
        cx
    end
  end

  # An approved declassification: its copy out, a crossing of this domain.
  def open_copy_out(cx) do
    case Guards.request(cx) do
      %{content: {:declassify, _, item}} = r ->
        {c, cx} = new_crossing(cx, :copy_out, item, r.snapshot || "", r.reader)
        raise_(cx, {:open, cx.domain, c})

      _ ->
        cx
    end
  end

  # An approved push: its write, a crossing of the target's domain.
  def open_write(cx) do
    case Guards.request(cx) do
      %{content: {:push, source, _, item}} = r ->
        {c, cx} = new_crossing(cx, :write, item, r.snapshot || "", source)
        raise_(cx, {:open, r.audit, c})

      _ ->
        cx
    end
  end

  # Crossings.

  # R42: a reader or writer budget with exactly the labelled side's labels and a deadline.
  def carve_crossing(cx) do
    cx = update(cx, :crossings, &%{&1 | through: true})
    limits = cx.store.fixed.sizes.crossing
    deadline = add(cx.event.now, @crossing_life)
    step(cx, {:create_budget, token(cx, @budget), :users, limits, elem(cx.domain, 1), deadline})
  end

  defp through(cx) do
    if match?(%{through: true}, Guards.crossing(cx)), do: token(cx, @budget)
  end

  def read_item(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> step(cx, {:read, token(cx, @read), through(cx), elem(cx.domain, 1), c.item})
    end
  end

  def write_item(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> step(cx, {:write, through(cx), elem(cx.domain, 1), c.item, {:literal, c.bytes}})
    end
  end

  def destroy_crossing(cx) do
    case through(cx) do
      nil -> cx
      t -> step(cx, {:destroy_budget, t})
    end
  end

  # R42: the copy out writes exactly the snapshot to the unlabelled volume, and reads nothing.
  def copy_out(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> step(cx, {:write, nil, [], c.item, {:literal, c.bytes}})
    end
  end

  # What the batch's steps made: the first bytes read, and the first budget's kernel id.
  defp produced(%{result: {:ok, list}}) do
    bytes = Enum.find_value(list, &(match?({:bytes, _}, &1) && elem(&1, 1)))
    budget = Enum.find_value(list, &(match?({:budget, _}, &1) && elem(&1, 1)))
    {bytes, budget || 0}
  end

  defp produced(_), do: {nil, 0}

  def pass_snapshot(cx) do
    case Guards.crossing(cx) do
      nil ->
        cx

      c ->
        {bytes, reader} = produced(cx)
        raise_(cx, {:snapshot, c.request, {bytes || "", reader}})
    end
  end

  def pass_failure(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> raise_(cx, {:snapshot, c.request, nil})
    end
  end

  def audit_declassified(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> audit(cx, {:declassified, elem(c.request, 2), c.bytes, c.source})
    end
  end

  def audit_copy_failed(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> audit(cx, {:copy_failed, elem(c.request, 2)})
    end
  end

  def audit_pushed(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> audit(cx, {:pushed, elem(c.request, 2), c.source, c.item, c.bytes, elem(produced(cx), 1)})
    end
  end

  def audit_push_failed(cx) do
    case Guards.crossing(cx) do
      nil -> cx
      c -> audit(cx, {:push_failed, elem(c.request, 2)})
    end
  end

  # Blame (R40).

  # Keeps the latest three blames' times.
  def count_blame(cx) do
    update_in(cx.store.domains[cx.domain].blame.times, fn t ->
      Enum.take(t ++ [cx.event.now], -Guards.blame_count())
    end)
  end

  def audit_blamed(cx), do: audit(cx, :blamed)

  # Every session and lease of the domain ends, and none starts until the window passes.
  def lock_out(cx) do
    cx =
      update_in(cx.store.domains[cx.domain].blame, fn b ->
        %{b | until: add(cx.event.now, Guards.blame_window_us()), times: []}
      end)

    st = Guards.state(cx)
    sessions = st.sessions |> Map.keys() |> Enum.sort() |> Enum.map(&{cx.domain, :session, &1})
    leases = st.leases |> Map.keys() |> Enum.sort() |> Enum.map(&{cx.domain, :lease, &1})
    Enum.reduce(sessions ++ leases, cx, &raise_(&2, {:locked_out, &1}))
  end

  def audit_locked_out(cx), do: audit(cx, {:locked_out, Guards.state(cx).blame.until})
end
