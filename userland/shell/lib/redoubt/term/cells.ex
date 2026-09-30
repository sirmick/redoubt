defmodule Redoubt.Term.Cells do
  @moduledoc """
  Reads a frame of cells, as the screen buffer's diff or a native program with a screen gives
  them: the one thing either may hand the session to draw (userland/native/cells).

  This is where a hijacked screen program meets the session, so it is strict, and keeps the same
  rules as the `cells` crate that writes frames; `userland/native/cells/vectors.json` holds both
  to the same answers. A symbol cannot hold a control character (C0, DEL, C1, or a bidirectional
  control that would reorder what is shown), a cell cannot fall outside its screen or be given
  twice, and a frame must be exactly its bytes. Anything else is refused, not repaired.
  """

  import Bitwise

  @max_side 1024
  @max_symbol 32
  @modifiers (1 <<< 9) - 1
  # The fewest bytes a cell takes, whatever its symbol: a position, a length, two colours and
  # modifiers. A count the bytes cannot hold is refused before any cell is read.
  @min_cell 2 + 2 + 1 + 4 + 4 + 2

  @type color :: :reset | {:indexed, 0..255} | {:rgb, 0..255, 0..255, 0..255}
  @type cell :: %{
          x: non_neg_integer(),
          y: non_neg_integer(),
          symbol: String.t(),
          fg: color(),
          bg: color(),
          modifiers: non_neg_integer()
        }
  @type frame :: %{width: pos_integer(), height: pos_integer(), clear: boolean(), cells: [cell()]}
  @type error ::
          :truncated
          | :version
          | :flags
          | :size
          | :count
          | :position
          | :twice
          | :symbol
          | :color
          | :modifiers
          | :trailing

  @doc """
  The frame in `bytes`, or why they are not exactly one. Fields are read in order, each checked
  as soon as it is read, as the Rust decoder reads them, so the two refuse a frame for the same
  reason.
  """
  @spec decode(binary()) :: {:ok, frame()} | {:error, error()}
  def decode(bytes) when is_binary(bytes) do
    with {:ok, version, rest} <- u8(bytes),
         :ok <- check(version == 1, :version),
         {:ok, flags, rest} <- u8(rest),
         :ok <- check((flags &&& bnot(1)) == 0, :flags),
         {:ok, width, rest} <- u16(rest),
         {:ok, height, rest} <- u16(rest),
         :ok <- check(width in 1..@max_side and height in 1..@max_side, :size),
         {:ok, count, rest} <- u32(rest),
         :ok <- check(count <= width * height, :count),
         :ok <- check(count <= div(byte_size(rest), @min_cell), :truncated) do
      cells(rest, count, {width, height}, MapSet.new(), [], %{width: width, height: height, clear: flags == 1})
    end
  end

  defp cells(<<>>, 0, _size, _seen, acc, frame), do: {:ok, Map.put(frame, :cells, Enum.reverse(acc))}
  defp cells(_rest, 0, _size, _seen, _acc, _frame), do: {:error, :trailing}

  defp cells(bytes, n, {width, height} = size, seen, acc, frame) do
    with {:ok, x, rest} <- u16(bytes),
         {:ok, y, rest} <- u16(rest),
         :ok <- inside(x, y, width, height),
         :ok <- once(seen, {x, y}),
         {:ok, length, rest} <- u8(rest),
         {:ok, symbol, rest} <- symbol(rest, length),
         {:ok, fg, rest} <- color(rest),
         {:ok, bg, rest} <- color(rest),
         {:ok, modifiers, rest} <- modifiers(rest) do
      cell = %{x: x, y: y, symbol: symbol, fg: fg, bg: bg, modifiers: modifiers}
      cells(rest, n - 1, size, MapSet.put(seen, {x, y}), [cell | acc], frame)
    end
  end

  defp u8(<<value, rest::binary>>), do: {:ok, value, rest}
  defp u8(_short), do: {:error, :truncated}
  defp u16(<<value::little-16, rest::binary>>), do: {:ok, value, rest}
  defp u16(_short), do: {:error, :truncated}
  defp u32(<<value::little-32, rest::binary>>), do: {:ok, value, rest}
  defp u32(_short), do: {:error, :truncated}

  defp check(true, _error), do: :ok
  defp check(false, error), do: {:error, error}

  defp inside(x, y, width, height), do: if(x < width and y < height, do: :ok, else: {:error, :position})
  defp once(seen, at), do: if(MapSet.member?(seen, at), do: {:error, :twice}, else: :ok)

  defp symbol(bytes, length) when byte_size(bytes) < length, do: {:error, :truncated}

  defp symbol(bytes, length) do
    <<symbol::binary-size(^length), rest::binary>> = bytes

    if length in 1..@max_symbol and String.valid?(symbol) and not forbidden?(symbol),
      do: {:ok, symbol, rest},
      else: {:error, :symbol}
  end

  @doc """
  Whether `text` holds a character no symbol may: C0, DEL, C1, or a bidirectional embedding,
  override or isolate control.
  """
  @spec forbidden?(String.t()) :: boolean()
  def forbidden?(text), do: text |> String.to_charlist() |> Enum.any?(&Redoubt.Term.Text.control?/1)

  defp color(<<0, 0, 0, 0, rest::binary>>), do: {:ok, :reset, rest}
  defp color(<<1, i, 0, 0, rest::binary>>), do: {:ok, {:indexed, i}, rest}
  defp color(<<2, r, g, b, rest::binary>>), do: {:ok, {:rgb, r, g, b}, rest}
  defp color(<<_color::binary-4, _rest::binary>>), do: {:error, :color}
  defp color(_short), do: {:error, :truncated}

  defp modifiers(<<bits::little-16, rest::binary>>) when (bits &&& bnot(@modifiers)) == 0,
    do: {:ok, bits, rest}

  defp modifiers(<<_bits::little-16, _rest::binary>>), do: {:error, :modifiers}
  defp modifiers(_short), do: {:error, :truncated}
end
