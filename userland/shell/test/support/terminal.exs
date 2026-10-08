defmodule Redoubt.Test.Terminal do
  @moduledoc """
  A model of the terminal the encoder draws on, for the tests to judge: a grid of cells, each
  with its style, and a cursor, fed the bytes the driver writes, as a VT102 with xterm's common
  extensions would take them.

  It understands exactly what `Redoubt.Term` writes: printable text, CR, LF, BS and BEL; relative
  and absolute cursor movement; erasing to the end of the screen and the whole screen; SGR's
  attributes and its 16, 256 and 24-bit colours; and the private modes for the alternate screen
  (1049) and the cursor's visibility (25). Anything else is a control sequence the encoder must
  never write, and raises. It translates nothing: a bare LF keeps its column, as Redoubt's
  console leaves it.
  """

  alias Redoubt.Term.Width

  @plain %{fg: :reset, bg: :reset, modifiers: MapSet.new()}

  defstruct cols: 80,
            rows: 24,
            cells: %{},
            styles: %{},
            row: 0,
            col: 0,
            pending: false,
            style: @plain,
            bells: 0,
            cursor: true,
            # The main screen while the alternate one is shown: {cells, styles, row, col}.
            saved: nil

  def new(cols, rows), do: %__MODULE__{cols: cols, rows: rows}

  @doc "Takes bytes the encoder wrote."
  def feed(terminal, bytes), do: parse(terminal, IO.iodata_to_binary(bytes))

  @doc "The screen's rows as strings, trailing blanks trimmed and trailing blank rows dropped."
  def lines(terminal) do
    for row <- 0..(terminal.rows - 1) do
      for col <- 0..(terminal.cols - 1), into: "", do: Map.get(terminal.cells, {row, col}, " ")
    end
    |> Enum.map(&String.trim_trailing/1)
    |> Enum.reverse()
    |> Enum.drop_while(&(&1 == ""))
    |> Enum.reverse()
  end

  @doc "The screen as one string, a line per row."
  def text(terminal), do: terminal |> lines() |> Enum.join("\n")

  @doc "Where the cursor is, `{row, col}`; at the margin it sits in the last column."
  def cursor(%{pending: true} = terminal), do: {terminal.row, terminal.cols - 1}
  def cursor(terminal), do: {terminal.row, terminal.col}

  @doc "The style a cell was drawn in: `%{fg, bg, modifiers}`, the modifiers a set of names."
  def style(terminal, row, col), do: Map.get(terminal.styles, {row, col}, @plain)

  @doc "Whether the cell was drawn with the bold attribute on."
  def bold?(terminal, row, col), do: MapSet.member?(style(terminal, row, col).modifiers, :bold)

  @doc "Whether the alternate screen is shown."
  def alternate?(terminal), do: terminal.saved != nil

  defp parse(terminal, <<>>), do: terminal
  defp parse(terminal, <<"\e[?", rest::binary>>), do: private(terminal, rest, "")
  defp parse(terminal, <<"\e[", rest::binary>>), do: csi(terminal, rest, "")

  defp parse(_terminal, <<"\e", rest::binary>>) do
    raise "a control sequence the encoder does not write reached the terminal: ESC #{inspect(binary_part(rest, 0, min(byte_size(rest), 8)))}"
  end

  defp parse(terminal, <<"\r", rest::binary>>), do: parse(%{terminal | col: 0, pending: false}, rest)
  defp parse(terminal, <<"\n", rest::binary>>), do: parse(down(%{terminal | pending: false}), rest)

  defp parse(terminal, <<"\b", rest::binary>>) do
    terminal = settle(terminal)
    parse(%{terminal | col: max(terminal.col - 1, 0)}, rest)
  end

  defp parse(terminal, <<"\a", rest::binary>>), do: parse(%{terminal | bells: terminal.bells + 1}, rest)

  defp parse(_terminal, <<c, _rest::binary>>) when c < 0x20 or c == 0x7F do
    raise "a control character reached the terminal: #{inspect(<<c>>)}"
  end

  defp parse(terminal, text) do
    {grapheme, rest} = String.next_grapheme(text)

    if grapheme |> String.to_charlist() |> Enum.any?(&Redoubt.Term.Text.control?/1) do
      raise "a control character reached the terminal: #{inspect(grapheme)}"
    end

    parse(put(terminal, grapheme), rest)
  end

  # A grapheme at the cursor: wrapping first when the cursor is pending at the margin or the
  # grapheme is too wide for what is left of the row.
  defp put(terminal, grapheme) do
    width = Width.columns(grapheme)

    terminal =
      if terminal.pending or terminal.col + width > terminal.cols,
        do: down(%{terminal | col: 0, pending: false}),
        else: terminal

    at = {terminal.row, terminal.col}
    cells = Map.put(terminal.cells, at, grapheme)
    styles = Map.put(terminal.styles, at, terminal.style)
    cells = if width == 2, do: Map.put(cells, {terminal.row, terminal.col + 1}, ""), else: cells
    col = terminal.col + width
    %{terminal | cells: cells, styles: styles, col: min(col, terminal.cols), pending: col >= terminal.cols}
  end

  defp down(terminal) do
    if terminal.row + 1 >= terminal.rows, do: scroll(terminal), else: %{terminal | row: terminal.row + 1}
  end

  defp scroll(terminal) do
    up = fn map -> for {{row, col}, v} <- map, row > 0, into: %{}, do: {{row - 1, col}, v} end
    %{terminal | cells: up.(terminal.cells), styles: up.(terminal.styles)}
  end

  # A cursor pending at the margin is in the last column once anything but text moves it.
  defp settle(%{pending: true} = terminal), do: %{terminal | col: terminal.cols - 1, pending: false}
  defp settle(terminal), do: terminal

  defp csi(_terminal, <<>>, params),
    do: raise("an unfinished control sequence reached the terminal: ESC [ #{params}")

  defp csi(terminal, <<c, rest::binary>>, params) when c in ?0..?9 or c == ?;,
    do: csi(terminal, rest, params <> <<c>>)

  defp csi(terminal, <<final, rest::binary>>, params) do
    args =
      params
      |> String.split(";")
      |> Enum.map(fn
        "" -> nil
        n -> String.to_integer(n)
      end)

    terminal = settle(terminal)

    terminal =
      case {final, args} do
        {?A, [n]} ->
          %{terminal | row: max(terminal.row - (n || 1), 0)}

        {?B, [n]} ->
          %{terminal | row: min(terminal.row + (n || 1), terminal.rows - 1)}

        {?C, [n]} ->
          %{terminal | col: min(terminal.col + (n || 1), terminal.cols - 1)}

        {?D, [n]} ->
          %{terminal | col: max(terminal.col - (n || 1), 0)}

        {?H, [nil]} ->
          %{terminal | row: 0, col: 0}

        {?H, [r, c]} ->
          %{terminal | row: clamp(r - 1, terminal.rows), col: clamp(c - 1, terminal.cols)}

        {?J, [nil]} ->
          erase_below(terminal)

        {?J, [2]} ->
          %{terminal | cells: %{}, styles: %{}}

        {?m, args} ->
          %{terminal | style: sgr(terminal.style, args, params)}

        _other ->
          raise "a control sequence the encoder does not write reached the terminal: ESC [ #{params}#{<<final>>}"
      end

    parse(terminal, rest)
  end

  defp clamp(n, size), do: n |> max(0) |> min(size - 1)

  # The private modes: the alternate screen and the cursor's visibility.
  defp private(terminal, <<c, rest::binary>>, params) when c in ?0..?9,
    do: private(terminal, rest, params <> <<c>>)

  defp private(terminal, <<final, rest::binary>>, params) do
    terminal =
      case {params, final} do
        {"1049", ?h} ->
          saved = {terminal.cells, terminal.styles, terminal.row, terminal.col}
          %{terminal | saved: saved, cells: %{}, styles: %{}, row: 0, col: 0, pending: false}

        {"1049", ?l} ->
          {cells, styles, row, col} = terminal.saved || raise("left an alternate screen never entered")
          %{terminal | saved: nil, cells: cells, styles: styles, row: row, col: col, pending: false}

        {"25", ?l} ->
          %{terminal | cursor: false}

        {"25", ?h} ->
          %{terminal | cursor: true}

        _other ->
          raise "a private mode the encoder does not set reached the terminal: ESC [ ? #{params}#{<<final>>}"
      end

    parse(terminal, rest)
  end

  @sgr %{1 => :bold, 2 => :dim, 3 => :italic, 4 => :underlined, 5 => :slow_blink, 6 => :rapid_blink}
  @sgr Map.merge(@sgr, %{7 => :reversed, 8 => :hidden, 9 => :crossed_out})

  defp sgr(_style, [nil], _params), do: @plain
  defp sgr(style, [], _params), do: style
  defp sgr(_style, [0 | rest], params), do: sgr(@plain, rest, params)

  defp sgr(style, [n | rest], params) when is_map_key(@sgr, n),
    do: sgr(%{style | modifiers: MapSet.put(style.modifiers, @sgr[n])}, rest, params)

  defp sgr(style, [n | rest], params) when n in 30..37,
    do: sgr(%{style | fg: {:indexed, n - 30}}, rest, params)

  defp sgr(style, [n | rest], params) when n in 90..97,
    do: sgr(%{style | fg: {:indexed, n - 82}}, rest, params)

  defp sgr(style, [n | rest], params) when n in 40..47,
    do: sgr(%{style | bg: {:indexed, n - 40}}, rest, params)

  defp sgr(style, [n | rest], params) when n in 100..107,
    do: sgr(%{style | bg: {:indexed, n - 92}}, rest, params)

  defp sgr(style, [38, 5, i | rest], params), do: sgr(%{style | fg: {:indexed, i}}, rest, params)
  defp sgr(style, [48, 5, i | rest], params), do: sgr(%{style | bg: {:indexed, i}}, rest, params)
  defp sgr(style, [38, 2, r, g, b | rest], params), do: sgr(%{style | fg: {:rgb, r, g, b}}, rest, params)
  defp sgr(style, [48, 2, r, g, b | rest], params), do: sgr(%{style | bg: {:rgb, r, g, b}}, rest, params)

  defp sgr(_style, _args, params),
    do: raise("an SGR the encoder does not write reached the terminal: ESC [ #{params} m")

  defp erase_below(terminal) do
    before = fn {row, col} -> row < terminal.row or (row == terminal.row and col < terminal.col) end
    cells = for {at, g} <- terminal.cells, before.(at), into: %{}, do: {at, g}
    styles = for {at, s} <- terminal.styles, before.(at), into: %{}, do: {at, s}
    %{terminal | cells: cells, styles: styles}
  end
end
