defmodule Redoubt.Term.Keys do
  @moduledoc """
  The key decoder: bytes typed at a terminal as keys (docs/userland/shell.md, "The terminal
  library"). A key is `{:key, key, modifiers}`: `key` a grapheme (`"a"`, `"世"`) or a name
  (`:enter`, `:tab`, `:backspace`, `:esc`, `:up`, `:down`, `:left`, `:right`, `:home`, `:end`,
  `:page_up`, `:page_down`, `:insert`, `:delete`, `{:f, n}`), and `modifiers` a list of `:shift`,
  `:alt` and `:ctrl`.

  It reads the VT100, xterm and Linux console spellings: CSI and SS3 cursor keys, `CSI n ~`
  editing and function keys, xterm's modifier parameter (`ESC [1;5C` is Ctrl+Right), the Linux
  console's `ESC [[A` for F1, Ctrl+letter as its control byte, and Alt as ESC before a key. An
  escape sequence it does not know is dropped whole, never taken as the characters in it.

  It never waits and reads no clock. Bytes that may be the start of a longer sequence (a lone ESC,
  `ESC [` with no final byte, half a UTF-8 character) come back as pending, for the caller to
  hand back with the next bytes, or to `flush/1` when it decides nothing more is coming: a lone
  ESC is then the Esc key. So the Esc timeout is the caller's, and a test needs no wall time.
  Only a screen in front reads keys through it; at the prompt, `edlin` decodes its own.
  """

  @type key :: {:key, String.t() | atom() | {:f, pos_integer()}, [:shift | :alt | :ctrl]}

  @doc "The keys in `bytes`, and the bytes that may begin a key not yet complete."
  @spec decode(binary()) :: {[key()], binary()}
  def decode(bytes), do: decode(bytes, [])

  defp decode(<<>>, acc), do: {Enum.reverse(acc), <<>>}

  defp decode(bytes, acc) do
    case one(bytes) do
      {:key, key, rest} -> decode(rest, [key | acc])
      {:skip, rest} -> decode(rest, acc)
      :pending -> {Enum.reverse(acc), bytes}
    end
  end

  @doc """
  The keys in `pending` once nothing more is coming: a lone ESC is Esc, and an unfinished
  sequence after it is Esc and the keys of what followed.
  """
  @spec flush(binary()) :: [key()]
  def flush(<<>>), do: []
  def flush(<<27, rest::binary>>), do: [{:key, :esc, []} | flush_rest(rest)]
  # Half a UTF-8 character that never completed: it is not a key.
  def flush(_partial), do: []

  defp flush_rest(rest) do
    {keys, pending} = decode(rest)
    keys ++ flush(pending)
  end

  # ---- one key ----

  # ESC: a sequence, Alt and a key, or (alone, so far) pending.
  defp one(<<27>>), do: :pending
  defp one(<<27, ?[, rest::binary>>), do: csi(rest)
  defp one(<<27, ?O, rest::binary>>), do: ss3(rest)
  defp one(<<27, rest::binary>>), do: alt(rest)

  defp one(<<c, rest::binary>>) when c in [?\r, ?\n], do: {:key, {:key, :enter, []}, rest}
  defp one(<<?\t, rest::binary>>), do: {:key, {:key, :tab, []}, rest}
  defp one(<<c, rest::binary>>) when c in [0x7F, 0x08], do: {:key, {:key, :backspace, []}, rest}
  defp one(<<0, rest::binary>>), do: {:key, {:key, " ", [:ctrl]}, rest}
  defp one(<<c, rest::binary>>) when c in 1..26, do: {:key, {:key, <<c + 96>>, [:ctrl]}, rest}
  defp one(<<c, rest::binary>>) when c in 28..31, do: {:key, {:key, <<c + 64>>, [:ctrl]}, rest}

  defp one(<<_, after_byte::binary>> = bytes) do
    {g, rest} = String.next_grapheme(bytes)

    cond do
      String.valid?(g) -> {:key, {:key, g, []}, rest}
      incomplete?(bytes) -> :pending
      # A byte that is not UTF-8 is not a key.
      true -> {:skip, after_byte}
    end
  end

  # Half a UTF-8 character at the end of what came.
  defp incomplete?(bytes), do: match?({:incomplete, _, _}, :unicode.characters_to_binary(bytes))

  # Alt and the key after ESC; an ESC with nothing after it yet is pending.
  defp alt(rest) do
    case one(rest) do
      {:key, {:key, key, mods}, rest} -> {:key, {:key, key, Enum.uniq([:alt | mods])}, rest}
      other -> other
    end
  end

  # ---- CSI: ESC [ parameters intermediates final ----

  # The Linux console's F1 to F5: ESC [ [ A..E.
  defp csi(<<?[, f, rest::binary>>) when f in ?A..?E, do: {:key, {:key, {:f, f - ?A + 1}, []}, rest}
  defp csi(<<?[>>), do: :pending

  defp csi(rest) do
    case split_csi(rest, <<>>) do
      {:ok, params, final, rest} ->
        case csi_key(params, final) do
          nil -> {:skip, rest}
          key -> {:key, key, rest}
        end

      :pending ->
        :pending
    end
  end

  # Parameter and intermediate bytes, then the final byte.
  defp split_csi(<<c, rest::binary>>, params) when c in 0x20..0x3F, do: split_csi(rest, params <> <<c>>)
  defp split_csi(<<final, rest::binary>>, params) when final in 0x40..0x7E, do: {:ok, params, final, rest}
  defp split_csi(<<>>, _params), do: :pending
  # A byte no sequence holds ends it: dropped, and read again as what it is.
  defp split_csi(<<_c, _::binary>> = rest, _params), do: {:ok, "", 0, rest}

  @cursor %{?A => :up, ?B => :down, ?C => :right, ?D => :left, ?H => :home, ?F => :end}

  @tilde %{
    1 => :home,
    2 => :insert,
    3 => :delete,
    4 => :end,
    5 => :page_up,
    6 => :page_down,
    7 => :home,
    8 => :end,
    11 => {:f, 1},
    12 => {:f, 2},
    13 => {:f, 3},
    14 => {:f, 4},
    15 => {:f, 5},
    17 => {:f, 6},
    18 => {:f, 7},
    19 => {:f, 8},
    20 => {:f, 9},
    21 => {:f, 10},
    23 => {:f, 11},
    24 => {:f, 12}
  }

  defp csi_key(params, final) do
    numbers = numbers(params)

    case {final, numbers} do
      {?Z, _} -> {:key, :tab, [:shift]}
      {f, n} when is_map_key(@cursor, f) and n != :bad -> {:key, @cursor[f], modifiers(n)}
      {?~, [k | _] = n} when is_map_key(@tilde, k) -> {:key, @tilde[k], modifiers(n)}
      _ -> nil
    end
  end

  # ESC [ 1 ; m: the modifier parameter is the second number, one more than the bits.
  defp modifiers([_, m | _]) when m >= 2, do: bits(m - 1)
  defp modifiers(_), do: []

  defp bits(b),
    do: for({bit, mod} <- [{1, :shift}, {2, :alt}, {4, :ctrl}], Bitwise.band(b, bit) != 0, do: mod)

  # The parameters as numbers, an empty one 1; a parameter that is not a number spoils them all.
  defp numbers(""), do: []

  defp numbers(params) do
    numbers =
      for param <- String.split(params, ";") do
        case Integer.parse(param) do
          {n, ""} -> n
          :error -> 1
          _bad -> :bad
        end
      end

    if :bad in numbers, do: :bad, else: numbers
  end

  # ---- SS3: ESC O final ----

  @ss3 Map.merge(@cursor, %{?P => {:f, 1}, ?Q => {:f, 2}, ?R => {:f, 3}, ?S => {:f, 4}})

  defp ss3(<<>>), do: :pending
  defp ss3(<<f, rest::binary>>) when is_map_key(@ss3, f), do: {:key, {:key, @ss3[f], []}, rest}
  defp ss3(<<_f, rest::binary>>), do: {:skip, rest}
end
