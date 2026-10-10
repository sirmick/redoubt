defmodule Redoubt.Term.Cells do
  @moduledoc """
  Reads a frame of cells, as the screen buffer's diff or a native program with a screen gives
  them: the one thing either may hand the session to draw (userland/native/cells).

  This is where a hijacked screen program meets the session, so it is strict, and keeps the same
  rules as the `cells` crate that writes frames; `userland/native/cells/vectors.json` holds both
  to the same answers. A symbol cannot hold a control character (C0, DEL, C1, or a bidirectional
  control that would reorder what is shown), a cell cannot fall outside its screen or be given
  twice, and a frame must be exactly its bytes. Anything else is refused, not repaired.

  A native program with a screen and the session exchange **records**, a `u32` length and that
  many bytes (`split/2`): on its standard output, each one frame of at most `max_frame/2` bytes;
  on its standard input, each one event the session writes (`event/1`), its size or a key.
  `userland/native/cells/events.json` holds `event/1` to the crate's event decoder.
  """

  import Bitwise

  @max_side 1024
  @max_symbol 32
  @modifiers (1 <<< 9) - 1
  # A frame's header: version, flags, width, height and count.
  @header 1 + 1 + 2 + 2 + 4
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
          | :length

  @doc """
  The frame in `bytes`, or why they are not exactly one. Fields are read in order, each checked
  as soon as it is read, as the Rust decoder reads them, so the two refuse a frame for the same
  reason. It is `reduce/3` collecting the cells.
  """
  @spec decode(binary()) :: {:ok, frame()} | {:error, error()}
  def decode(bytes) when is_binary(bytes) do
    collect = fn
      {:frame, frame}, nil -> {:cont, {frame, []}}
      {:cell, cell}, {frame, cells} -> {:cont, {frame, [cell | cells]}}
    end

    case reduce(bytes, nil, collect) do
      {:ok, {frame, cells}} -> {:ok, Map.put(frame, :cells, Enum.reverse(cells))}
      {:error, _} = error -> error
    end
  end

  @doc """
  The decoder, a cell at a time, so that a large frame is never held as one term: `fun` is given
  `{:frame, %{width:, height:, clear:}}` once the header is read and checked, then `{:cell, cell}`
  for each cell as it is read and checked, and returns `{:cont, acc}` or `{:halt, value}`. Gives
  `{:ok, acc}` when the bytes are exactly one frame, `{:halt, value}` when `fun` stopped, or
  `{:error, why}` at the first field refused, as `decode/1` would refuse it: by then `fun` has
  seen the cells before it, and a caller draws nothing until it has `{:ok, acc}`.
  """
  @spec reduce(binary(), acc, ({:frame, map()} | {:cell, cell()}, acc -> {:cont, acc} | {:halt, term()})) ::
          {:ok, acc} | {:halt, term()} | {:error, error()}
        when acc: term()
  def reduce(bytes, acc, fun) when is_binary(bytes) and is_function(fun, 2) do
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
      case fun.({:frame, %{width: width, height: height, clear: flags == 1}}, acc) do
        {:cont, acc} -> cells(rest, count, {width, height}, MapSet.new(), acc, fun)
        {:halt, _value} = halt -> halt
      end
    end
  end

  defp cells(<<>>, 0, _size, _seen, acc, _fun), do: {:ok, acc}
  defp cells(_rest, 0, _size, _seen, _acc, _fun), do: {:error, :trailing}

  defp cells(bytes, n, {width, height} = size, seen, acc, fun) do
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

      case fun.({:cell, cell}, acc) do
        {:cont, acc} -> cells(rest, n - 1, size, MapSet.put(seen, {x, y}), acc, fun)
        {:halt, _value} = halt -> halt
      end
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

  # ---- records and events ----

  @doc """
  The most bytes a frame of a `cols` by `rows` screen can take: every cell given, each with the
  longest symbol. A record claiming more is refused before any of it is held.
  """
  @spec max_frame(pos_integer(), pos_integer()) :: pos_integer()
  def max_frame(cols, rows), do: @header + cols * rows * (@min_cell + @max_symbol)

  @doc """
  The first record in `bytes` and what follows it, `:more` while it has not all arrived, or
  `{:error, :length}` as soon as its length says more than `max`.
  """
  @spec split(binary(), non_neg_integer()) :: {:ok, binary(), binary()} | :more | {:error, :length}
  def split(<<length::little-32, _::binary>>, max) when length > max, do: {:error, :length}
  def split(<<length::little-32, body::binary-size(length), rest::binary>>, _max), do: {:ok, body, rest}
  def split(_bytes, _max), do: :more

  @doc "`body` as a record: its length, then its bytes."
  @spec record(binary()) :: binary()
  def record(body), do: <<byte_size(body)::little-32, body::binary>>

  # The named keys, in the order of their codes from 1.
  @named ~w(enter tab backspace esc up down left right home end page_up page_down insert delete)a
  @key_modifiers %{shift: 1, alt: 2, ctrl: 4}

  @doc """
  An event's bytes, a record's body, as a native program with a screen reads it: `{:size, cols,
  rows}`, or a key as `Redoubt.Term.Keys` gives it, `{:key, key, modifiers}`. A key's symbol
  follows the frame's rule, so no control character is ever one; anything not an event is
  `{:error, reason}`.

  The interrupt is never a program's key: Ctrl+\\, the session's own key, with any other
  modifiers, is `{:error, :interrupt}`, and so is Ctrl+C unless `opts` has `ctrl_c: :key`, the
  screen having been given it.
  """
  @spec event(term(), keyword()) ::
          {:ok, binary()} | {:error, :size | :symbol | :key | :modifiers | :interrupt}
  def event(event, opts \\ [])

  def event({:size, cols, rows}, _opts) when cols in 1..@max_side and rows in 1..@max_side,
    do: {:ok, <<1, cols::little-16, rows::little-16>>}

  def event({:size, _cols, _rows}, _opts), do: {:error, :size}

  def event({:key, key, modifiers}, opts) when is_list(modifiers) do
    with :ok <- not_interrupt(key, modifiers, Keyword.get(opts, :ctrl_c, :interrupt)),
         {:ok, bits} <- key_modifiers(modifiers),
         {:ok, code} <- key_code(key),
         do: {:ok, <<2, bits, code::binary>>}
  end

  def event(_other, _opts), do: {:error, :key}

  defp not_interrupt(key, modifiers, ctrl_c) do
    interrupt = key == "\\" or (key == "c" and ctrl_c != :key)
    if interrupt and :ctrl in modifiers, do: {:error, :interrupt}, else: :ok
  end

  defp key_modifiers(modifiers) do
    if Enum.all?(modifiers, &is_map_key(@key_modifiers, &1)),
      do: {:ok, modifiers |> Enum.uniq() |> Enum.map(&@key_modifiers[&1]) |> Enum.sum()},
      else: {:error, :modifiers}
  end

  defp key_code(symbol) when is_binary(symbol) do
    if byte_size(symbol) in 1..@max_symbol and String.valid?(symbol) and not forbidden?(symbol),
      do: {:ok, <<0, byte_size(symbol), symbol::binary>>},
      else: {:error, :symbol}
  end

  defp key_code({:f, n}) when n in 1..24, do: {:ok, <<0x80 + n>>}

  defp key_code(name) do
    case Enum.find_index(@named, &(&1 == name)) do
      nil -> {:error, :key}
      i -> {:ok, <<i + 1>>}
    end
  end
end
