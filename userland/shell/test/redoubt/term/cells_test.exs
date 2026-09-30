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
    assert length(cases) == 33

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
end
