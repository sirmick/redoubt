# Runs vectors/example.txt through the generated Elixir codec: the same vectors as the Rust
# codec (redoubt/wire/tests/vectors.rs). Run on beamlet by redoubt/wire/elixir/run-vectors,
# which roots the VM's file system at redoubt/wire/vectors.
defmodule Redoubt.Wire.VectorsTest do
  import Bitwise
  alias Redoubt.Wire.Proto.Example

  @doc "Returns `{:ok, oks, bads}`, or `{:failed, failures}` listing each failing line."
  def start(path \\ "/example.txt") do
    results =
      File.read!(path)
      |> String.split("\n")
      |> Enum.reject(&(String.trim(&1) == "" or String.starts_with?(&1, "#")))
      |> Enum.map(fn line -> {line, check(String.split(line))} end)

    refusals =
      for message <- hostile_messages(),
          not match?({:error, _}, Example.encode(message)),
          do: {inspect(message), {:fail, :encoded}}

    case for({line, {:fail, why}} <- results ++ refusals, do: {line, why}) do
      [] ->
        {:ok, Enum.count(results, &match?({_, :ok}, &1)), Enum.count(results, &match?({_, :bad}, &1))}

      failures ->
        {:failed, failures}
    end
  end

  defp check([kind, handles, w0, w1, w2, w3, buffer | rest]) do
    words = Enum.map([w0, w1, w2, w3], &String.to_integer(&1, 16))
    buffer = if buffer == "-", do: <<>>, else: Base.decode16!(buffer, case: :lower)
    decoded = Example.decode(words, buffer, String.to_integer(handles))

    case {kind, decoded} do
      {"ok", {:ok, {name, fields} = message}} ->
        rendered = Enum.join([Atom.to_string(name) | render(name, fields)], " ")
        len = if buffer == <<>>, do: 0, else: Enum.at(words, 1)

        cond do
          rendered != Enum.join(rest, " ") -> {:fail, {:fields, rendered}}
          Example.encode(message) != {:ok, words, binary_part(buffer, 0, len)} -> {:fail, {:encode, Example.encode(message)}}
          true -> :ok
        end

      {"ok", error} ->
        {:fail, error}

      {"bad", {:error, _}} ->
        :bad

      {"bad", accepted} ->
        {:fail, {:accepted, accepted}}
    end
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

  # FIELD=VALUE in the table's order, rendered as in the vector file.
  defp render(name, fields) do
    {_opcode, _shape, layout, _handles} = Example.layout(name)

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
  end
end
