# The approval screen and the request binding hash (docs/servers/steward.md, "The powerbox and
# approvals", R38, and "Guards and effects"), for the Elixir reference.
defmodule Redoubt.Steward.Render do
  @field_cap 64
  @declassify_max 256

  # Printable ASCII only, every other character shown as `?`, cut to `cap` characters, with `"`
  # and `\` escaped so a field cannot end its own quoting.
  def sanitize(s, cap) do
    s
    |> chars()
    |> Enum.take(cap)
    |> Enum.map(fn
      c when c in [?", ?\\] -> <<?\\, c>>
      c when c in 0x20..0x7E -> <<c>>
      _ -> "?"
    end)
    |> Enum.join()
  end

  # The characters of UTF-8 text; a byte that is not part of valid UTF-8 is one character of its
  # own, which sanitize shows as `?`.
  defp chars(<<>>), do: []
  defp chars(<<c::utf8, rest::binary>>), do: [c | chars(rest)]
  defp chars(<<_, rest::binary>>), do: [0xFFFD | chars(rest)]

  # A lease in human units: "2 h 5 min", "90 s", "250 ms".
  def human(us) do
    {h, m, s, ms} =
      {div(us, 3_600_000_000), rem(div(us, 60_000_000), 60), rem(div(us, 1_000_000), 60), rem(div(us, 1000), 1000)}

    case {h, m, s} do
      {0, 0, 0} -> "#{ms} ms"
      {0, 0, s} -> "#{s} s"
      {0, m, _} -> "#{m} min"
      {h, m, _} -> "#{h} h #{m} min"
    end
  end

  # A label list as the page's screens write it: [7, 9].
  defp list(l), do: "[" <> Enum.map_join(l, ", ", &Integer.to_string/1) <> "]"

  defp hex(b), do: Base.encode16(b, case: :lower)

  # The screen for request `r` of `domain`: who asks (kind and steward-assigned name, beside its
  # principal), what, for how long, and the label consequences. A labelled requester's free text
  # is withheld: it could carry the vault out.
  def screen(fixed, {_, labels} = _domain, st, r) do
    labelled = labels != []
    who = sanitize(Enum.at(fixed.principals, r.principal).name, @field_cap)

    {kind, number} =
      case r.by do
        {:lease, id} -> {"agent", (st.leases[id] && st.leases[id].number) || 0}
        {_, id} -> {"session", (st.sessions[id] && st.sessions[id].number) || 0}
      end

    what =
      case r.content do
        {:agent, l, lease} ->
          "start an agent labelled #{list(l)} for #{human(lease)}"

        {:declassify, l, item} ->
          shown = if r.snapshot, do: sanitize(r.snapshot, @declassify_max), else: ""
          "declassify item #{item} of labels #{list(l)}: \"#{shown}\""

        {:push, source, target, item} ->
          b = r.snapshot || ""

          "push unlabelled item #{source} (#{byte_size(b)} bytes, sha256 #{hex(:crypto.hash(:sha256, b))}) " <>
            "to item #{item} of labels #{list(target)}"

        {:note, _} when labelled ->
          "a note (text withheld: labelled requester)"

        {:note, w} ->
          "a note (untrusted): \"#{sanitize(w, @field_cap)}\""
      end

    reason =
      if labelled,
        do: "reason withheld (labelled requester)",
        else: "reason (untrusted): \"#{sanitize(r.reason, @field_cap)}\""

    text = "request from #{who} (#{kind}-#{number}, labels #{list(labels)}): #{what}; #{reason}"
    %{id: r.id, hash: r.hash, labels: labels, text: text}
  end

  defp le(n), do: <<n::little-64>>
  defp words(l), do: [le(length(l)) | Enum.map(l, &le/1)]
  defp bytes(b), do: [le(byte_size(b)), b]

  # SHA-256 over the request's canonical encoding: its requester's domain, its id, what it asks
  # for, the snapshot it froze and its reason. Any change makes another hash.
  def binding({account, labels}, r) do
    content =
      case r.content do
        {:note, w} -> [<<0>>, bytes(w)]
        {:agent, l, lease} -> [<<1>>, words(l), le(lease)]
        {:declassify, l, item} -> [<<2>>, words(l), le(item)]
        {:push, source, target, item} -> [<<3>>, le(source), words(target), le(item)]
      end

    snapshot = if r.snapshot == nil, do: <<0>>, else: [<<1>>, bytes(r.snapshot)]
    data = ["redoubt.steward.request.v1", <<0>>, le(account), words(labels), le(r.id), content, snapshot, bytes(r.reason)]
    :crypto.hash(:sha256, IO.iodata_to_binary(data))
  end
end
