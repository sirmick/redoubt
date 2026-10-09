defmodule Redoubt.Term.CellsTest do
  use ExUnit.Case, async: true

  alias Redoubt.Term.Cells

  # The cases the cells crate writes, and holds its own decoder to: this decoder must answer the
  # same. On beamlet ./test-shell mounts them read-only and says where (CELLS_VECTORS).
  defp vectors do
    (System.get_env("CELLS_VECTORS") || Path.expand("../../../../native/cells/vectors.json", __DIR__))
    |> File.read!()
    |> JSON.decode!()
  end

  defp color(["reset"]), do: :reset
  defp color(["indexed", i]), do: {:indexed, i}
  defp color(["rgb", r, g, b]), do: {:rgb, r, g, b}

  defp frame(%{"width" => w, "height" => h, "clear" => clear, "cells" => cells}) do
    cells =
      for c <- cells,
          do: %{
            x: c["x"],
            y: c["y"],
            symbol: c["symbol"],
            fg: color(c["fg"]),
            bg: color(c["bg"]),
            modifiers: c["modifiers"]
          }

    %{width: w, height: h, clear: clear, cells: cells}
  end

  test "every case of the cells crate is answered as it answers it" do
    cases = vectors()
    assert length(cases) == 34

    for %{"name" => name, "hex" => hex} = vector <- cases do
      bytes = Base.decode16!(hex, case: :lower)

      case vector do
        %{"frame" => expected} -> assert Cells.decode(bytes) == {:ok, frame(expected)}, name
        %{"error" => error} -> assert Cells.decode(bytes) == {:error, String.to_existing_atom(error)}, name
      end
    end
  end

  test "no decoded symbol ever holds a control character, whatever the bytes" do
    good =
      vectors() |> Enum.find(&Map.has_key?(&1, "frame")) |> then(&Base.decode16!(&1["hex"], case: :lower))

    :rand.seed(:exsss, {7, 11, 13})

    for _ <- 1..5_000 do
      bytes =
        Enum.reduce(1..:rand.uniform(4), good, fn _, b ->
          at = :rand.uniform(byte_size(b)) - 1
          <<head::binary-size(^at), _byte, tail::binary>> = b
          head <> <<:rand.uniform(256) - 1>> <> tail
        end)

      case Cells.decode(bytes) do
        {:ok, frame} -> refute Enum.any?(frame.cells, &Cells.forbidden?(&1.symbol))
        {:error, reason} -> assert is_atom(reason)
      end
    end
  end

  test "the forbidden characters are C0, DEL, C1 and the direction controls" do
    for c <- Enum.concat([0..0x1F, 0x7F..0x9F, 0x202A..0x202E, 0x2066..0x2069]) do
      assert Cells.forbidden?(<<c::utf8>>), "U+#{Integer.to_string(c, 16)}"
    end

    for ok <- ["a", " ", "é", "界", "🙂", "👩\u200D💻", "\u00A0", "\u2028"], do: refute(Cells.forbidden?(ok))
  end

  # The events the cells crate's decoder reads, beside its frames' vectors.
  defp events do
    (System.get_env("CELLS_VECTORS") || Path.expand("../../../../native/cells/vectors.json", __DIR__))
    |> Path.dirname()
    |> Path.join("events.json")
    |> File.read!()
    |> JSON.decode!()
  end

  defp event(%{"size" => [cols, rows]}), do: {:size, cols, rows}

  defp event(%{"key" => key, "modifiers" => mods}) do
    key =
      case key do
        ["f", n] -> {:f, n}
        [name] -> String.to_existing_atom(name)
        symbol -> symbol
      end

    {:key, key, Enum.map(mods, &String.to_existing_atom/1)}
  end

  test "every event the cells crate reads is written as it reads it" do
    good = for %{"event" => e, "hex" => hex, "name" => name} <- events(), do: {name, event(e), hex}
    assert length(good) == 24

    for {name, event, hex} <- good do
      assert Cells.event(event, ctrl_c: :key) == {:ok, Base.decode16!(hex, case: :lower)}, name
    end
  end

  test "no event carries a control character or anything that is not a key" do
    for c <- [0x1C, 0x1B, 0x03, 0x00, 0x7F, 0x9B, 0x202E] do
      assert Cells.event({:key, <<c::utf8>>, []}) == {:error, :symbol}
      assert Cells.event({:key, "a" <> <<c::utf8>>, [:ctrl]}) == {:error, :symbol}
    end

    assert Cells.event({:key, "", []}) == {:error, :symbol}
    assert Cells.event({:key, String.duplicate("x", 33), []}) == {:error, :symbol}
    assert Cells.event({:key, <<0xFF>>, []}) == {:error, :symbol}
    assert Cells.event({:key, :menu, []}) == {:error, :key}
    assert Cells.event({:key, {:f, 25}, []}) == {:error, :key}
    assert Cells.event({:key, "a", [:meta]}) == {:error, :modifiers}
    assert Cells.event({:size, 0, 24}) == {:error, :size}
    assert Cells.event({:size, 80, 1025}) == {:error, :size}
    assert Cells.event(:paste) == {:error, :key}
  end

  test "the interrupt is no program's key: Ctrl+\\ never, Ctrl+C only when the screen was given it" do
    for mods <- [[:ctrl], [:ctrl, :alt], [:shift, :ctrl]] do
      assert Cells.event({:key, "\\", mods}) == {:error, :interrupt}
      assert Cells.event({:key, "\\", mods}, ctrl_c: :key) == {:error, :interrupt}
      assert Cells.event({:key, "c", mods}) == {:error, :interrupt}
      assert {:ok, _} = Cells.event({:key, "c", mods}, ctrl_c: :key)
    end

    assert {:ok, _} = Cells.event({:key, "\\", []})
    assert {:ok, _} = Cells.event({:key, "c", [:alt]})
  end

  test "a record is its length and its bytes, and one longer than its bound is refused unread" do
    {:ok, frame} = Cells.decode(Base.decode16!(hd(vectors())["hex"], case: :lower))
    body = Base.decode16!(hd(vectors())["hex"], case: :lower)
    stream = Cells.record(body) <> Cells.record("next")
    assert {:ok, ^body, rest} = Cells.split(stream, Cells.max_frame(80, 24))
    assert Cells.decode(body) == {:ok, frame}
    assert Cells.split(rest, 4) == {:ok, "next", ""}

    for n <- 0..(4 + byte_size(body) - 1),
        do: assert(Cells.split(binary_part(stream, 0, n), Cells.max_frame(80, 24)) == :more)

    assert Cells.split(<<0xFFFFFFFF::little-32>>, Cells.max_frame(1024, 1024)) == {:error, :length}
    assert Cells.split(Cells.record("12345"), 4) == {:error, :length}
    assert Cells.max_frame(80, 24) == 10 + 80 * 24 * 47
  end

  test "reduce reads every case as decode does, a cell at a time, and stops when told" do
    for %{"name" => name, "hex" => hex} <- vectors() do
      bytes = Base.decode16!(hex, case: :lower)
      events = Cells.reduce(bytes, [], fn event, acc -> {:cont, [event | acc]} end)

      case Cells.decode(bytes) do
        {:ok, frame} ->
          assert {:ok, [{:frame, header} | cells]} = events |> then(fn {:ok, e} -> {:ok, Enum.reverse(e)} end),
                 name

          assert header == Map.delete(frame, :cells), name
          assert Enum.map(cells, fn {:cell, c} -> c end) == frame.cells, name

        {:error, _} = error ->
          assert events == error, name
      end
    end

    good =
      vectors() |> Enum.find(&(&1["name"] =~ "each colour")) |> then(&Base.decode16!(&1["hex"], case: :lower))

    assert Cells.reduce(good, 0, fn _, _ -> {:halt, :stopped} end) == {:halt, :stopped}

    assert Cells.reduce(good, 0, fn
             {:frame, _}, n -> {:cont, n}
             {:cell, _}, 1 -> {:halt, :second}
             {:cell, _}, n -> {:cont, n + 1}
           end) == {:halt, :second}
  end
end
