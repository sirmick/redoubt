defmodule Redoubt.Screen.ReadTest do
  # A native program's frame read into the screen buffer by the natives (`Redoubt.Screen.Native.read/3`,
  # the decoder in Rust) is held to its reference (`reference/3`, the decoder in Elixir): the
  # same answer and the same buffer, for every vector of the cells crate and for seeded
  # mutations of a whole screen's frame.
  use ExUnit.Case, async: true

  alias Redoubt.Screen.Native
  alias Redoubt.Term.Buffer

  unless Buffer.available?() do
    @moduletag skip: "the screen buffer is beamlet's natives, which the BEAM does not have"
  end

  # The cases the cells crate writes (as in cells_test.exs).
  defp vectors do
    (System.get_env("CELLS_VECTORS") || Path.expand("../../../../native/cells/vectors.json", __DIR__))
    |> File.read!()
    |> JSON.decode!()
  end

  # Both readings of `bytes`, each into a fresh buffer of `size` drawn on and diffed first: their
  # answers, and what each buffer then holds. Each in a process of its own, which holds its
  # buffer until it ends (a process holds at most four).
  defp both(bytes, {cols, rows} = size) do
    for read <- [&Native.read/3, &Native.reference/3] do
      Task.await(
        Task.async(fn ->
          b = Buffer.new(cols, rows)
          Buffer.put(b, 0, 0, "before", Buffer.style(fg: {:indexed, 1}))
          Buffer.diff(b)
          {read.(b, size, bytes), Buffer.diff(b)}
        end)
      )
    end
  end

  test "every vector is answered as the decoder answers it, by both readings alike" do
    cases = vectors()
    assert length(cases) == 34

    for %{"name" => name, "hex" => hex} = vector <- cases do
      bytes = Base.decode16!(hex, case: :lower)

      # A buffer of the frame's size, when a buffer may be that size; else any, since a header
      # the decoder refuses is refused before its size is compared.
      size =
        case bytes do
          <<_, _, w::little-16, h::little-16, _::binary>>
          when w in 1..1024 and h in 1..1024 and w * h <= 65_536 ->
            {w, h}

          _other ->
            {80, 24}
        end

      [{answer, diff}, {reference, reference_diff}] = both(bytes, size)
      assert answer == reference, name
      assert diff == reference_diff, name

      case vector do
        %{"frame" => %{"width" => w, "height" => h}} when {w, h} != size ->
          assert answer == {:size, w, h}, name

        %{"frame" => _} ->
          assert answer == :ok, name

        %{"error" => error} ->
          assert answer == {:error, String.to_existing_atom(error)}, name
      end
    end
  end

  # One cell's bytes, as the cells crate writes them.
  defp cell(x, y, symbol, fg, modifiers),
    do:
      <<x::little-16, y::little-16, byte_size(symbol), symbol::binary, fg::binary-4, 0::32,
        modifiers::little-16>>

  # A whole 80 by 24 frame: plain symbols, wide ones, ones joining code points (`joined`, then an
  # emoji with its selector, a flag, a family), every colour kind, and a wide symbol at column
  # `edge` of the last row, after the row's other cells.
  defp whole(edge, joined \\ "e\u0301") do
    symbols = ["a", "界", joined, "❤\uFE0F", "🇫🇷", "👩‍👩‍👧", "─", "Z"]
    colors = [<<0, 0, 0, 0>>, <<1, 4, 0, 0>>, <<2, 1, 2, 3>>]

    {cells, n} =
      for y <- 0..23, reduce: {[], 0} do
        acc ->
          room = if y == 23, do: 78, else: 80

          Stream.cycle(symbols)
          |> Enum.reduce_while({acc, 0}, fn s, {{cells, n}, x} ->
            w = Redoubt.Term.Width.grapheme(s)

            if x + w > room,
              do: {:halt, {cells, n}},
              else:
                {:cont, {{[cell(x, y, s, Enum.at(colors, rem(n, 3)), rem(n, 512)) | cells], n + 1}, x + w}}
          end)
      end

    cells = Enum.reverse([cell(edge, 23, "界", <<0::32>>, 0) | cells])
    <<1, 1, 80::little-16, 24::little-16, n + 1::little-32>> <> IO.iodata_to_binary(cells)
  end

  test "a whole screen's frame, and seeded mutations of it, read alike into alike buffers" do
    # The edge cell with room (78), then without (79); a byte after the frame; a symbol of two
    # graphemes.
    fits = whole(78)
    :rand.seed(:exsss, {17, 19, 23})

    inputs =
      [fits, whole(79), fits <> <<0>>, whole(78, "ab")] ++
        for _ <- 1..400 do
          Enum.reduce(1..:rand.uniform(4), fits, fn _, b ->
            at = :rand.uniform(byte_size(b)) - 1
            <<head::binary-size(^at), _byte, tail::binary>> = b
            head <> <<:rand.uniform(256) - 1>> <> tail
          end)
          |> then(&binary_part(&1, 0, byte_size(&1) - (:rand.uniform(3) - 1)))
        end

    answers =
      for bytes <- inputs do
        [{answer, diff}, {reference, reference_diff}] = both(bytes, {80, 24})
        assert answer == reference
        assert diff == reference_diff
        answer
      end

    assert Enum.take(answers, 4) == [:ok, {:error, :cell}, {:error, :trailing}, {:error, :cell}]

    # The mutations reach the refusals that matter, not only the first field.
    reasons =
      answers
      |> Enum.map(fn
        {:error, why} -> why
        other -> other
      end)
      |> MapSet.new()

    for why <- [:ok, :position, :twice, :symbol, :color, :modifiers, :truncated],
        do: assert(why in reasons, inspect(why))
  end

  test "a frame's joined symbol is put only once OTP calls it one grapheme" do
    b = Buffer.new(4, 1)

    frame = fn symbol ->
      <<1, 0, 4::little-16, 1::little-16, 1::little-32>> <> cell(0, 0, symbol, <<0::32>>, 0)
    end

    assert Native.read(b, {4, 1}, frame.("é")) == :ok
    assert Native.read(b, {4, 1}, frame.("ab")) == {:error, :cell}
    assert {:ok, %{cells: [%{symbol: "é"}]}} = Redoubt.Term.Cells.decode(Buffer.diff(b))
  end
end
