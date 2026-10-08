defmodule Redoubt.Test.Terminal do
  @moduledoc """
  A model of the terminal the encoder draws on, for the tests to judge: a grid of cells and a
  cursor, fed the bytes the driver writes, as a VT102 would take them.

  It understands exactly what `Redoubt.Term` writes: printable text, CR, LF, BS and BEL,
  relative cursor movement, erasing to the end of the screen, and the bold and underline
  attributes. Anything else is a control sequence the encoder must never write, and raises. It
  translates nothing: a bare LF keeps its column, as Redoubt's console leaves it.
  """

  alias Redoubt.Term.Width

  defstruct cols: 80,
            rows: 24,
            cells: %{},
            bold: MapSet.new(),
            row: 0,
            col: 0,
            pending: false,
            attribute: false,
            bells: 0

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

  @doc "Whether the cell was drawn with the bold attribute on."
  def bold?(terminal, row, col), do: MapSet.member?(terminal.bold, {row, col})

  defp parse(terminal, <<>>), do: terminal
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

    cells = Map.put(terminal.cells, {terminal.row, terminal.col}, grapheme)
    cells = if width == 2, do: Map.put(cells, {terminal.row, terminal.col + 1}, ""), else: cells

    bold =
      if terminal.attribute, do: MapSet.put(terminal.bold, {terminal.row, terminal.col}), else: terminal.bold

    col = terminal.col + width
    %{terminal | cells: cells, bold: bold, col: min(col, terminal.cols), pending: col >= terminal.cols}
  end

  defp down(terminal) do
    if terminal.row + 1 >= terminal.rows, do: scroll(terminal), else: %{terminal | row: terminal.row + 1}
  end

  defp scroll(terminal) do
    cells = for {{row, col}, g} <- terminal.cells, row > 0, into: %{}, do: {{row - 1, col}, g}
    bold = for {row, col} <- terminal.bold, row > 0, into: MapSet.new(), do: {row - 1, col}
    %{terminal | cells: cells, bold: bold}
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

        {?J, [nil]} ->
          erase_below(terminal)

        {?J, [2]} ->
          %{terminal | cells: %{}, bold: MapSet.new()}

        {?H, [nil]} ->
          %{terminal | row: 0, col: 0}

        {?m, [nil]} ->
          %{terminal | attribute: false}

        {?m, [0]} ->
          %{terminal | attribute: false}

        {?m, _} ->
          %{terminal | attribute: true}

        _other ->
          raise "a control sequence the encoder does not write reached the terminal: ESC [ #{params}#{<<final>>}"
      end

    parse(terminal, rest)
  end

  defp erase_below(terminal) do
    before = fn {row, col} -> row < terminal.row or (row == terminal.row and col < terminal.col) end
    cells = for {at, g} <- terminal.cells, before.(at), into: %{}, do: {at, g}
    bold = for at <- terminal.bold, before.(at), into: MapSet.new(), do: at
    %{terminal | cells: cells, bold: bold}
  end
end
