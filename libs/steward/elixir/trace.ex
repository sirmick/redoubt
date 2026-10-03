# The reference's side of the steward core's traces (docs/servers/steward.md, "The trace
# encoding"): reads each trace, runs it through the reference, and writes the output in the
# canonical encoding the Rust side writes, for a byte-for-byte comparison, and a line `done` at
# the end. Run on beamlet with the traces' directory as the VM's root: `start/0` reads every
# `*.trace` there, and a file `break` there names a guard to replace with one that always holds
# (the negative run).
defmodule Redoubt.Steward.Trace do
  alias Redoubt.Steward

  def start(dir \\ "/") do
    broken =
      case File.read(Path.join(dir, "break")) do
        {:ok, g} -> String.to_atom(String.trim(g))
        _ -> nil
      end

    dir
    |> File.ls!()
    |> Enum.filter(&String.ends_with?(&1, ".trace"))
    |> Enum.sort()
    |> Enum.each(fn name ->
      {out, rows} = run(File.read!(Path.join(dir, name)), broken)
      IO.write(["trace ", name, "\n", out])
      rows |> Enum.sort() |> Enum.each(fn {m, l} -> IO.write("row #{m} #{l}\n") end)
    end)

    # The end of the output: what follows (beamlet prints start/0's value) is not the reference's.
    IO.write("done\n")
    :ok
  end

  # ---- Input -----------------------------------------------------------------------------

  # A line's tokens, split at spaces outside a quoted string; `#` outside a string starts a
  # comment.
  def tokens(line), do: tokens(line, [], "", :out)

  defp tokens(<<>>, _acc, _cur, :quoted), do: raise("unclosed string")
  defp tokens(<<>>, acc, cur, _), do: Enum.reverse(push(acc, cur))
  defp tokens(<<?\\, c, rest::binary>>, acc, cur, :quoted), do: tokens(rest, acc, cur <> <<?\\, c>>, :quoted)
  defp tokens(<<?", rest::binary>>, acc, cur, :quoted), do: tokens(rest, acc, cur <> "\"", :in)
  defp tokens(<<c, rest::binary>>, acc, cur, :quoted), do: tokens(rest, acc, cur <> <<c>>, :quoted)
  defp tokens(<<?\s, rest::binary>>, acc, cur, _), do: tokens(rest, push(acc, cur), "", :out)
  defp tokens(<<?#, _::binary>>, acc, "", _), do: Enum.reverse(acc)
  defp tokens(<<?", rest::binary>>, acc, cur, _), do: tokens(rest, acc, cur <> "\"", :quoted)
  defp tokens(<<c, rest::binary>>, acc, cur, _), do: tokens(rest, acc, cur <> <<c>>, :in)

  defp push(acc, ""), do: acc
  defp push(acc, t), do: [t | acc]

  defp fields(toks) do
    Map.new(toks, fn t ->
      [k, v] = String.split(t, "=", parts: 2)
      {k, v}
    end)
  end

  defp num(s), do: String.to_integer(s)

  defp list("[]"), do: []
  defp list("[" <> s), do: s |> String.trim_trailing("]") |> String.split(",") |> Enum.map(&num/1)

  defp lists("[" <> s) do
    inner = binary_part(s, 0, byte_size(s) - 1)
    Regex.scan(~r/\[[0-9,]*\]/, inner) |> Enum.map(fn [l] -> list(l) end)
  end

  defp triple(s), do: s |> String.split(",") |> Enum.map(&num/1) |> List.to_tuple()

  # A quoted string's bytes: `\"`, `\\`, `\n` and `\xNN` are the escapes.
  def unquote_(<<?", rest::binary>>), do: unesc(binary_part(rest, 0, byte_size(rest) - 1), [])

  defp unesc(<<>>, acc), do: acc |> Enum.reverse() |> IO.iodata_to_binary()
  defp unesc(<<?\\, ?n, r::binary>>, acc), do: unesc(r, [?\n | acc])
  defp unesc(<<?\\, ?x, h::binary-2, r::binary>>, acc), do: unesc(r, [String.to_integer(h, 16) | acc])
  defp unesc(<<?\\, c, r::binary>>, acc) when c in [?", ?\\], do: unesc(r, [c | acc])
  defp unesc(<<c, r::binary>>, acc), do: unesc(r, [c | acc])

  def domain(s) do
    [a, l] = String.split(s, "/", parts: 2)
    labels = if l == "", do: [], else: l |> String.split(",") |> Enum.map(&num/1)
    {num(a), Steward.labels(labels)}
  end

  @kinds ~w(session lease request crossing blame channel)

  def object(s) do
    [k, rest] = String.split(s, "@", parts: 2)
    {d, id} = rest |> String.split("#") |> then(&{Enum.join(Enum.drop(&1, -1), "#"), List.last(&1)})
    true = k in @kinds
    {domain(d), String.to_atom(k), num(id)}
  end

  defp produced("scope"), do: :scope
  defp produced("connection"), do: :connection
  defp produced("done"), do: :done
  defp produced("budget(" <> n), do: {:budget, num(String.trim_trailing(n, ")"))}
  defp produced("process(" <> n), do: {:process, num(String.trim_trailing(n, ")"))}
  defp produced("bytes(" <> b), do: {:bytes, unquote_(binary_part(b, 0, byte_size(b) - 1))}

  # `[budget(5),scope,bytes("a,b")]`: split at commas outside strings and parentheses.
  defp produced_list("[" <> s) do
    s |> binary_part(0, byte_size(s) - 1) |> split_items([], "", 0, false) |> Enum.map(&produced/1)
  end

  defp split_items(<<>>, acc, "", _, _), do: Enum.reverse(acc)
  defp split_items(<<>>, acc, cur, _, _), do: Enum.reverse([cur | acc])
  defp split_items(<<?\\, c, r::binary>>, acc, cur, d, true), do: split_items(r, acc, cur <> <<?\\, c>>, d, true)
  defp split_items(<<?", r::binary>>, acc, cur, d, q), do: split_items(r, acc, cur <> "\"", d, not q)
  defp split_items(<<?(, r::binary>>, acc, cur, d, false), do: split_items(r, acc, cur <> "(", d + 1, false)
  defp split_items(<<?), r::binary>>, acc, cur, d, false), do: split_items(r, acc, cur <> ")", d - 1, false)
  defp split_items(<<?,, r::binary>>, acc, cur, 0, false), do: split_items(r, [cur | acc], "", 0, false)
  defp split_items(<<c, r::binary>>, acc, cur, d, q), do: split_items(r, acc, cur <> <<c>>, d, q)

  defp content_of(f) do
    cond do
      Map.has_key?(f, "note") -> {:note, unquote_(f["note"])}
      Map.has_key?(f, "agent") -> {:agent, list(f["agent"]), num(f["lease"])}
      Map.has_key?(f, "declassify") -> {:declassify, list(f["declassify"]), num(f["item"])}
      Map.has_key?(f, "push") -> {:push, num(f["source"]), list(f["push"]), num(f["item"])}
    end
  end

  defp hash("shown"), do: :shown
  defp hash(h), do: Base.decode16!(h, case: :mixed)

  defp kind("Login", f), do: {:login, unquote_(f["principal"]), list(f["labels"]), num(f["key"])}
  defp kind("ChannelClosed", f), do: {:channel_closed, num(f["session"])}

  defp kind("ApprovalOpened", f),
    do: {:approval_opened, num(f["channel"]), unquote_(f["principal"]), num(f["key"])}

  defp kind("ApprovalClosed", f), do: {:approval_closed, num(f["channel"])}
  defp kind("StartAgent", f), do: {:start_agent, num(f["badge"]), num(f["lease"])}
  defp kind("Submit", f), do: {:submit, num(f["badge"]), content_of(f), unquote_(f["reason"])}
  defp kind("EndLease", f), do: {:end_lease, num(f["badge"]), num(f["lease"])}
  defp kind("EndSession", f), do: {:end_session, num(f["badge"])}
  defp kind("Pending", f), do: {:pending, num(f["channel"])}
  defp kind("Approve", f), do: {:approve, num(f["channel"]), num(f["request"]), hash(f["hash"])}
  defp kind("Deny", f), do: {:deny, num(f["channel"]), num(f["request"])}
  defp kind("Blame", f), do: {:blame, num(f["account"]), list(f["labels"])}
  defp kind("Exited", f), do: {:exited, object(f["object"])}

  defp kind("Done", f) do
    result =
      case f do
        %{"ok" => ok} ->
          {:ok, produced_list(ok)}

        %{"failed" => failed} ->
          [s, e] = String.split(failed, ",")
          {:error, num(s), num(e)}
      end

    {:done, object(f["object"]), result}
  end

  defp event(toks) do
    {head, [name | rest]} = Enum.split_while(toks, &String.contains?(&1, "="))
    h = fields(head)
    words = if h["random"], do: list(h["random"]), else: []
    random = words ++ List.duplicate(0, 8 - length(words))
    %{now: num(h["now"]), random: random, reply: num(h["reply"] || "0"), kind: kind(name, fields(rest))}
  end

  defp principal([name | rest]) do
    f = fields(rest)

    %{
      name: unquote_(name),
      account: num(f["account"]),
      login_keys: list(f["login"]),
      approval_keys: list(f["approval"]),
      owned: list(f["owned"]),
      label_sets: lists(f["sets"]),
      top: triple(f["top"])
    }
  end

  def parse(text) do
    m = %{principals: [], keyd_keys: [], servers: 0, sizes: nil}

    {m, events} =
      text
      |> String.split("\n")
      |> Enum.reduce({m, []}, fn line, {m, events} ->
        case tokens(String.trim_trailing(line, "\r")) do
          [] -> {m, events}
          ["principal" | rest] -> {%{m | principals: m.principals ++ [principal(rest)]}, events}
          ["keyd", l] -> {%{m | keyd_keys: list(l)}, events}
          ["servers", n] -> {%{m | servers: num(n)}, events}
          ["sizes" | rest] -> {%{m | sizes: sizes(fields(rest))}, events}
          ["event" | rest] -> {m, [event(rest) | events]}
        end
      end)

    {m, Enum.reverse(events)}
  end

  defp sizes(f) do
    %{
      session: triple(f["session"]),
      agent: triple(f["agent"]),
      sub_agent: triple(f["sub_agent"]),
      crossing: triple(f["crossing"]),
      budget_cost: num(f["cost"])
    }
  end

  # ---- Running ---------------------------------------------------------------------------

  def run(text, broken \\ nil) do
    {m, events} = parse(text)

    case Steward.boot(m, broken) do
      nil ->
        {"boot\nrefused\n", MapSet.new()}

      {store, carves} ->
        head = ["boot\n", boot(store, carves)]

        {store, _, out} =
          events
          |> Enum.with_index(1)
          |> Enum.reduce({store, %{}, [head]}, fn {e, i}, {store, shown, out} ->
            e =
              case e.kind do
                {:approve, c, r, :shown} -> %{e | kind: {:approve, c, r, Map.get(shown, r, <<0::256>>)}}
                _ -> e
              end

            {store, effects} = Steward.decide(store, e)

            shown =
              Enum.reduce(effects.outputs, shown, fn
                {:screen, _, s}, acc -> Map.put(acc, s.id, s.hash)
                _, acc -> acc
              end)

            {store, shown, [out, "event #{i}\n", effects(effects), "store\n", dump(store)]}
          end)

        {IO.iodata_to_binary(out), store.rows}
    end
  end

  # ---- Output ----------------------------------------------------------------------------

  def qs(b) do
    for <<c <- b>>, into: "\"" do
      case c do
        ?" -> "\\\""
        ?\\ -> "\\\\"
        ?\n -> "\\n"
        c when c in 0x20..0x7E -> <<c>>
        c -> "\\x" <> String.downcase(Base.encode16(<<c>>))
      end
    end <> "\""
  end

  defp hex(b), do: Base.encode16(b, case: :lower)
  defp list_(l), do: "[" <> Enum.map_join(l, ",", &Integer.to_string/1) <> "]"
  defp dom({a, l}), do: "#{a}/" <> Enum.map_join(l, ",", &Integer.to_string/1)
  defp obj({d, k, id}), do: "#{k}@#{dom(d)}##{id}"
  defp tok({o, slot}), do: "#{obj(o)}.#{slot}"
  defp lim({p, q, w}), do: "#{p},#{q},#{w}"
  defp opt(nil), do: "none"
  defp opt(x), do: "#{x}"

  defp camel(a), do: a |> Atom.to_string() |> String.split("_") |> Enum.map_join(&String.capitalize/1)

  defp boot(store, carves) do
    f = store.fixed

    principals =
      f.principals
      |> Enum.with_index()
      |> Enum.map(fn {p, i} ->
        "principal #{i} #{qs(p.name)} account=#{p.account} login=#{list_(p.login_keys)} " <>
          "approval=#{list_(p.approval_keys)} owned=#{list_(p.owned)} " <>
          "domains=[#{Enum.map_join(p.domains, ",", &dom/1)}] top=#{lim(p.top)}\n"
      end)

    z = f.sizes

    fixed =
      "fixed keyd=#{list_(f.keyd)} servers=#{f.servers} sizes session=#{lim(z.session)} agent=#{lim(z.agent)} " <>
        "sub_agent=#{lim(z.sub_agent)} crossing=#{lim(z.crossing)} cost=#{z.budget_cost}\n"

    c =
      Enum.map(carves, fn {account, top, subs} ->
        ["carve account=#{account} top=#{lim(top)}\n" | Enum.map(subs, fn {d, l} -> "carve-sub #{dom(d)} #{lim(l)}\n" end)]
      end)

    [principals, fixed, c]
  end

  defp answer(:ok), do: "ok"
  defp answer({:session, id, name}), do: "session id=#{id} name=#{qs(name)}"
  defp answer({:lease, id, name}), do: "lease id=#{id} name=#{qs(name)}"
  defp answer({:request, id}), do: "request id=#{id}"
  defp answer({:refused, why}), do: "refused #{why}"

  defp record({:login, s, p, k}), do: "Login session=#{s} principal=#{p} key=#{k}"

  defp record({:agent_started, l, s, p, d}),
    do: "AgentStarted lease=#{l} sponsor=#{s} parent=#{opt(p)} deadline=#{d}"

  defp record({:start_failed, l}), do: "StartFailed lease=#{l}"
  defp record({:lease_ended, l}), do: "LeaseEnded lease=#{l}"
  defp record({:submitted, r}), do: "Submitted request=#{r}"
  defp record({:approved, r, p, k, h}), do: "Approved request=#{r} principal=#{p} key=#{k} hash=#{hex(h)}"
  defp record({:denied, r, p}), do: "Denied request=#{r} principal=#{p}"
  defp record({:declassified, r, b, rd}), do: "Declassified request=#{r} bytes=#{qs(b)} reader=#{rd}"
  defp record({:copy_failed, r}), do: "CopyFailed request=#{r}"

  defp record({:pushed, r, s, i, b, w}),
    do: "Pushed request=#{r} source=#{s} item=#{i} bytes=#{qs(b)} writer=#{w}"

  defp record({:push_failed, r}), do: "PushFailed request=#{r}"
  defp record(:blamed), do: "Blamed"
  defp record({:locked_out, u}), do: "LockedOut until=#{u}"

  defp to({:session, b}), do: "session #{b}"
  defp to({:channel, c}), do: "channel #{c}"

  defp through(nil), do: "none"
  defp through(t), do: tok(t)

  defp step({:create_budget, t, parent, l, labels, deadline}) do
    p =
      case parent do
        {:sub, d} -> "sub(#{dom(d)})"
        {:budget, b} -> "budget(#{tok(b)})"
        :users -> "users"
      end

    "create-budget token=#{tok(t)} parent=#{p} limits=#{lim(l)} labels=#{list_(labels)} deadline=#{opt(deadline)}"
  end

  defp step({:create_scope, s, b}), do: "create-scope scope=#{tok(s)} budget=#{tok(b)}"

  defp step({:connect, t, s, server, badge}) do
    server = if server == :steward, do: "steward", else: "shared(#{elem(server, 1)})"
    "connect token=#{tok(t)} scope=#{tok(s)} server=#{server} badge=#{badge}"
  end

  defp step({:launch, t, b, c}),
    do: "launch token=#{tok(t)} budget=#{tok(b)} connections=[#{Enum.map_join(c, ",", &tok/1)}]"

  defp step({:destroy_budget, b}), do: "destroy-budget budget=#{tok(b)}"

  defp step({:read, t, th, l, item}),
    do: "read token=#{tok(t)} through=#{through(th)} labels=#{list_(l)} item=#{item}"

  defp step({:write, th, l, item, {:literal, b}}),
    do: "write through=#{through(th)} labels=#{list_(l)} item=#{item} bytes=literal(#{qs(b)})"

  defp output({:reply, t, a}), do: "reply #{t} #{answer(a)}\n"
  defp output({:notice, t, :approval_waiting}), do: "notice #{to(t)} approval-waiting\n"
  defp output({:notice, t, {:lease_ended, l}}), do: "notice #{to(t)} lease-ended lease=#{l}\n"

  defp output({:screen, ch, s}),
    do: "screen #{ch} id=#{s.id} hash=#{hex(s.hash)} labels=#{list_(s.labels)} text=#{qs(s.text)}\n"

  defp output({:audit, d, at, r}), do: "audit #{dom(d)} at=#{at} #{record(r)}\n"
  defp output({:forget, o}), do: "forget #{obj(o)}\n"

  defp effects(e) do
    outputs = Enum.map(e.outputs, &output/1)

    batches =
      Enum.map(e.batches, fn {o, steps} -> ["batch #{obj(o)}\n" | Enum.map(steps, &"step #{step(&1)}\n")] end)

    [outputs, batches, if(e.exit, do: "exit\n", else: "")]
  end

  defp content({:note, w}), do: "note(#{qs(w)})"
  defp content({:agent, l, lease}), do: "agent(#{list_(l)},#{lease})"
  defp content({:declassify, l, item}), do: "declassify(#{list_(l)},#{item})"
  defp content({:push, s, t, item}), do: "push(#{s},#{list_(t)},#{item})"

  defp crossing_kind(:read), do: "read"
  defp crossing_kind(:copy_out), do: "copy-out"
  defp crossing_kind(:write), do: "write"

  defp sorted(m), do: m |> Enum.sort() |> Enum.map(&elem(&1, 1))

  defp dump(store) do
    domains =
      Enum.map(store.order, fn d ->
        st = store.domains[d]
        b = st.blame

        [
          "domain #{dom(d)} sessions_started=#{st.sessions_started} agents_started=#{st.agents_started} " <>
            "blame=#{camel(b.state)} times=#{list_(b.times)} until=#{b.until}\n",
          for x <- sorted(st.sessions) do
            "  session id=#{x.id} state=#{camel(x.state)} principal=#{x.principal} key=#{x.key} " <>
              "badge=#{x.badge} number=#{x.number} reply=#{x.reply}\n"
          end,
          for x <- sorted(st.leases) do
            "  lease id=#{x.id} state=#{camel(x.state)} principal=#{x.principal} badge=#{x.badge} " <>
              "number=#{x.number} lease=#{x.lease} deadline=#{x.deadline} parent=#{opt(x.parent)} " <>
              "granted=#{x.granted} reply=#{x.reply}\n"
          end,
          for x <- sorted(st.requests) do
            {bk, bid} = x.by
            snapshot = if x.snapshot == nil, do: "none", else: qs(x.snapshot)

            "  request id=#{x.id} state=#{camel(x.state)} by=#{bk}##{bid} principal=#{x.principal} " <>
              "content=#{content(x.content)} reason=#{qs(x.reason)} snapshot=#{snapshot} reader=#{x.reader} " <>
              "hash=#{hex(x.hash)} audit=#{dom(x.audit)} channel=#{opt(x.channel)} reply=#{x.reply}\n"
          end,
          for x <- sorted(st.crossings) do
            "  crossing id=#{x.id} state=#{camel(x.state)} kind=#{crossing_kind(x.kind)} request=#{obj(x.request)} " <>
              "item=#{x.item} bytes=#{qs(x.bytes)} source=#{x.source} through=#{x.through}\n"
          end
        ]
      end)

    routes = for {b, {d, k, id}} <- Enum.sort(store.routes), do: "route #{b} #{k}@#{dom(d)}##{id}\n"
    ids = for {id, {d, k}} <- Enum.sort(store.ids), do: "id #{id} #{k}@#{dom(d)}\n"

    channels =
      for c <- sorted(store.channels),
          do: "channel id=#{c.id} state=#{camel(c.state)} principal=#{c.principal} key=#{c.key}\n"

    [domains, routes, ids, channels, "exited #{store.exited}\n"]
  end
end
