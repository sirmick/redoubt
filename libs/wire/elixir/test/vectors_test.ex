# Runs the typed-message vector files through the generated Elixir codec: the same files as
# the Rust codec (redoubt/wire/tests/vectors.rs). Run on the real BEAM and on beamlet by
# redoubt/wire/elixir/run-vectors, which compares the two outputs byte for byte.
defmodule Redoubt.Wire.VectorsTest do
  import Bitwise
  alias Redoubt.Wire.Proto.Example

  @doc """
  Checks each file in `dir` and prints one line per file:
  `vectors FILE: KIND=COUNT ... failures=N`, then one line per failure. Returns `:ok` or
  `:failed`.
  """
  def start(dir \\ "/") do
    results = Enum.map(["example.txt", "example-generated.txt"], &check_file(dir, &1))
    refusals = for m <- hostile_messages(), not match?({:error, _}, Example.encode(m)), do: inspect(m)
    IO.puts("vectors hostile encodes: #{length(hostile_messages())} failures=#{length(refusals)}")
    Enum.each(refusals, &IO.puts("  FAIL encoded #{&1}"))
    if Enum.all?(results) and refusals == [], do: :ok, else: :failed
  end

  defp check_file(dir, name) do
    results =
      File.read!(Path.join(dir, name))
      |> String.split("\n")
      |> Enum.reject(&(String.trim(&1) == "" or String.starts_with?(&1, "#")))
      |> Enum.map(fn line -> {line, check(String.split(line))} end)

    counts =
      results
      |> Enum.map(fn {line, _} -> hd(String.split(line)) end)
      |> Enum.frequencies()
      |> Enum.sort()
      |> Enum.map_join(" ", fn {k, v} -> "#{k}=#{v}" end)

    failures = for {line, {:fail, why}} <- results, do: {line, why}
    IO.puts("vectors #{name}: #{counts} failures=#{length(failures)}")
    Enum.each(failures, fn {line, why} -> IO.puts("  FAIL #{inspect(why)}: #{line}") end)
    failures == []
  end

  defp words(ws), do: Enum.map(ws, &String.to_integer(&1, 16))
  defp unhex("-"), do: <<>>
  defp unhex(hex), do: Base.decode16!(hex, case: :lower)

  # The first word-1 bytes of what arrived: what a buffer message re-encodes to.
  defp prefix(words, buffer), do: if(buffer == <<>>, do: <<>>, else: binary_part(buffer, 0, Enum.at(words, 1)))

  defp check(["ok", h, w0, w1, w2, w3, buf | rest]) do
    {ws, buffer} = {words([w0, w1, w2, w3]), unhex(buf)}

    case Example.decode(ws, buffer, String.to_integer(h)) do
      {:ok, {name, fields} = m} ->
        expect(render(name, fields, :request), rest, Example.encode(m), {:ok, ws, prefix(ws, buffer)})

      other ->
        {:fail, other}
    end
  end

  defp check(["bad", h, w0, w1, w2, w3, buf, _error]),
    do: refused(Example.decode(words([w0, w1, w2, w3]), unhex(buf), String.to_integer(h)))

  defp check([kind, op, h, w0, w1, w2, w3, buf | rest]) when kind in ["reply", "failed", "badreply"] do
    {ws, buffer} = {words([w0, w1, w2, w3]), unhex(buf)}

    case {kind, Example.decode_reply(String.to_integer(op, 16), ws, buffer, String.to_integer(h))} do
      {"reply", {:ok, {name, fields} = r}} ->
        expect(render(name, fields, :reply), rest, Example.encode_reply(r), {:ok, ws, prefix(ws, buffer)})

      {"failed", {:failed, error}} ->
        expect([Atom.to_string(error)], rest, Example.encode_error(error), {:ok, ws, <<>>})

      {"badreply", got} ->
        refused(got)

      {_, other} ->
        {:fail, other}
    end
  end

  defp check(["file", hex | rest]) do
    bytes = unhex(hex)

    case Example.decode_file(bytes) do
      {:ok, {name, fields} = m} -> expect(render(name, fields, :request), rest, Example.encode_file(m), {:ok, bytes})
      other -> {:fail, other}
    end
  end

  defp check(["badfile", hex, _error]), do: refused(Example.decode_file(unhex(hex)))

  defp expect(rendered, rest, encoded, want) do
    cond do
      rendered != rest -> {:fail, {:fields, Enum.join(rendered, " ")}}
      encoded != want -> {:fail, {:encode, encoded}}
      true -> :ok
    end
  end

  defp refused({:error, _}), do: :ok
  defp refused(accepted), do: {:fail, {:accepted, accepted}}

  # NAME FIELD=VALUE in the table's order, rendered as in the vector files.
  defp render(name, fields, dir) do
    {_opcode, _shape, request, _handles, reply} = Example.layout(name)
    layout = if dir == :request, do: request, else: elem(reply, 0)

    values =
      for {field, type} <- layout do
        value = Map.fetch!(fields, field)

        text =
          case type do
            :string -> "s:" <> Base.encode16(value, case: :lower)
            :bytes -> "b:" <> Base.encode16(value, case: :lower)
            _ -> Integer.to_string(value)
          end

        "#{field}=#{text}"
      end

    [Atom.to_string(name) | values]
  end

  # Messages the encoder must refuse (the Rust types cannot express these at all).
  defp hostile_messages do
    [
      {:pong, %{seq: -1, flags: 0}},
      {:pong, %{seq: 1 <<< 64, flags: 0}},
      {:small, %{a: 256, b: 0}},
      {:small, %{a: 1.0, b: 0}},
      {:small, %{a: 1}},
      {:small, %{a: 1, b: 2, c: 3}},
      {:named, %{id: 1, name: <<0xFF>>}},
      {:named, %{id: 1, name: :binary.copy("a", 65536)}},
      {:named, %{id: 1, name: ~c"list"}},
      {:blob, %{offset: 0, data: :binary.copy("a", 65536), label: ""}},
      {:nope, %{}},
      :ping,
      {:ping, []}
    ]
  end
end
