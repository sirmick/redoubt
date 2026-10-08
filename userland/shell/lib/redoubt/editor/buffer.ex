defmodule Redoubt.Editor.Buffer do
  @moduledoc """
  The editor's text (docs/userland/shell.md, "The editor"): a file's lines around a cursor, with
  a selection, undo and redo, and find and replace. Pure data: it reads and writes nothing.

  A line is a binary without its newline; the text is the lines joined by `"\\n"`, so a file read
  and written back unedited is the same bytes, its final newline (or the lack of one) and any
  `\\r` included. The lines above the cursor's are held reversed, the cursor's apart, and those
  below in order, so an edit at the cursor costs the line it is on. A position is
  `{row, col}`, both from 0, `col` counted in graphemes.

  Every edit can be undone, one step at a time; characters typed one after another at the
  cursor are one step. Undo keeps at most 500 steps, and fewer when they hold much: a step holds
  the lines the cursor crossed since the step before it (crossing them builds their list again)
  and its own copy of the cursor's line, counted as a line for every 16 bytes; once the steps
  hold more than `undo_lines/0` the oldest go. The newest is always kept. Find and replace match a pattern within one line at a time: a
  `Regex`, linear in time on beamlet (docs/userland/beamlet.md), or a string matched as it is.
  """

  defstruct above: [],
            line: "",
            below: [],
            row: 0,
            col: 0,
            size: 1,
            goal: nil,
            anchor: nil,
            undo: [],
            redo: [],
            clock: 0,
            version: 0,
            saved: 0,
            last: nil,
            relinked: 0

  @type t :: %__MODULE__{}
  @type pos :: {non_neg_integer(), non_neg_integer()}

  # How many steps undo keeps.
  @depth 500

  # How many lines undo's steps may hold between them: a line crossed costs a list cell of two
  # words or so, so this keeps undo near 2M words, an eighth of a screen's heap (Redoubt.Screen).
  @undo_lines 1_000_000

  # The bytes of the cursor's line a step counts as one line: about what a line crossed costs.
  @line_bytes 16

  @doc """
  How many lines undo's steps hold at most, beyond the newest step's: the lines crossed or
  inserted, and a line for every 16 bytes of the cursor's line each step keeps.
  """
  @spec undo_lines() :: pos_integer()
  def undo_lines, do: @undo_lines

  @doc "A buffer holding `text`, the cursor at its start, nothing to undo, and unmodified."
  @spec new(String.t()) :: t()
  def new(text \\ "") do
    [line | below] = String.split(text, "\n")
    %__MODULE__{line: line, below: below, size: length(below) + 1}
  end

  @doc "The whole text."
  @spec text(t()) :: String.t()
  def text(b), do: b |> lines() |> Enum.intersperse("\n") |> IO.iodata_to_binary()

  @doc "Every line, in order."
  @spec lines(t()) :: [String.t()]
  def lines(b), do: Enum.reverse(b.above, [b.line | b.below])

  @doc "The lines from row `first`, at most `n` of them: what a view of `n` rows shows."
  @spec slice(t(), non_neg_integer(), non_neg_integer()) :: [String.t()]
  def slice(b, first, n) do
    above = if first < b.row, do: b.above |> Enum.take(b.row - first) |> Enum.reverse(), else: []
    below = Enum.take(b.below, max(first + n - b.row - 1, 0))
    (above ++ [b.line | below]) |> Enum.drop(max(first - b.row, 0)) |> Enum.take(n)
  end

  @doc "The number of lines."
  @spec size(t()) :: pos_integer()
  def size(b), do: b.size

  @doc "The cursor."
  @spec cursor(t()) :: pos()
  def cursor(b), do: {b.row, b.col}

  @doc "The cursor's line."
  @spec line(t()) :: String.t()
  def line(b), do: b.line

  @doc "The selection, its start and its end, in order, or `nil` when nothing is selected."
  @spec selection(t()) :: {pos(), pos()} | nil
  def selection(%{anchor: nil}), do: nil
  def selection(%{anchor: anchor} = b) when anchor == {b.row, b.col}, do: nil
  def selection(b), do: Enum.min_max([b.anchor, {b.row, b.col}])

  @doc "Whether the text differs from what it was when last marked saved, or made."
  @spec modified?(t()) :: boolean()
  def modified?(b), do: b.version != b.saved

  @doc "Marks the text as saved as it is now."
  @spec mark_saved(t()) :: t()
  def mark_saved(b), do: %{b | saved: b.version}

  # ---- moving ----

  @doc """
  Moves the cursor: `:left`, `:right`, `:up`, `:down`, `:home`, `:end`, `:top`, `:bottom`,
  `:word_left`, `:word_right`, `{:page, rows}` (negative is up) or `{:to, row, col}`. With
  `select: true` the selection grows from where it started; without, it goes.
  """
  @spec move(t(), term(), keyword()) :: t()
  def move(b, where, opts \\ []) do
    anchor = if Keyword.get(opts, :select, false), do: b.anchor || {b.row, b.col}, else: nil
    b = %{step(b, where) | anchor: anchor, last: nil}
    if where in [:up, :down] or match?({:page, _}, where), do: b, else: %{b | goal: nil}
  end

  defp step(b, :left) when b.col > 0, do: %{b | col: b.col - 1}
  defp step(%{row: 0} = b, :left), do: b
  defp step(b, :left), do: b |> to_row(b.row - 1) |> then(&%{&1 | col: String.length(&1.line)})

  defp step(b, :right) do
    cond do
      b.col < String.length(b.line) -> %{b | col: b.col + 1}
      b.below == [] -> b
      true -> %{to_row(b, b.row + 1) | col: 0}
    end
  end

  defp step(b, :up), do: vertical(b, -1)
  defp step(b, :down), do: vertical(b, 1)
  defp step(b, {:page, rows}), do: vertical(b, rows)
  defp step(b, :home), do: %{b | col: 0}
  defp step(b, :end), do: %{b | col: String.length(b.line)}
  defp step(b, :top), do: %{to_row(b, 0) | col: 0}
  defp step(b, :bottom), do: b |> to_row(b.size - 1) |> then(&%{&1 | col: String.length(&1.line)})
  defp step(b, {:to, row, col}), do: b |> to_row(row) |> clamp(col)
  defp step(b, :word_left), do: word(b, -1)
  defp step(b, :word_right), do: word(b, 1)

  # Up or down by `n` rows, keeping the column the move started from where the line allows.
  defp vertical(b, n) do
    goal = b.goal || b.col
    %{(b |> to_row(b.row + n) |> clamp(goal)) | goal: goal}
  end

  defp clamp(b, col), do: %{b | col: col |> max(0) |> min(String.length(b.line))}

  # The cursor to `row`, clamped to the text: the lines it passes move from one side to the other.
  defp to_row(b, row) do
    row = row |> max(0) |> min(b.size - 1)

    cond do
      row == b.row ->
        b

      row < b.row ->
        {moved, [line | above]} = Enum.split(b.above, b.row - row - 1)
        below = Enum.reverse(moved, [b.line | b.below])
        %{b | above: above, line: line, below: below, row: row, relinked: b.relinked + b.row - row}

      true ->
        {moved, [line | below]} = Enum.split(b.below, row - b.row - 1)
        above = Enum.reverse(moved, [b.line | b.above])
        %{b | above: above, line: line, below: below, row: row, relinked: b.relinked + row - b.row}
    end
  end

  # To the start of the next word (`1`), or of this or the last one (`-1`), across lines.
  defp word(b, dir) do
    graphemes = String.graphemes(b.line)

    case word_col(graphemes, b.col, dir) do
      nil when dir < 0 and b.row > 0 -> step(b, :left)
      nil when dir > 0 and b.below != [] -> step(b, :right)
      nil -> b
      col -> %{b | col: col}
    end
  end

  defp word_col(graphemes, col, 1) do
    rest = Enum.drop(graphemes, col)
    skip = rest |> Enum.take_while(&word?/1) |> length()
    gap = rest |> Enum.drop(skip) |> Enum.take_while(&(not word?(&1))) |> length()
    if col + skip + gap > col and col < length(graphemes), do: col + skip + gap, else: nil
  end

  defp word_col(graphemes, col, -1) do
    before = graphemes |> Enum.take(col) |> Enum.reverse()
    gap = before |> Enum.take_while(&(not word?(&1))) |> length()
    skip = before |> Enum.drop(gap) |> Enum.take_while(&word?/1) |> length()
    if col > 0, do: col - gap - skip, else: nil
  end

  defp word?(g), do: g =~ ~r/^[\p{L}\p{N}_]+$/u

  @doc "Selects the whole text, the cursor at its end."
  @spec select_all(t()) :: t()
  def select_all(b), do: %{step(b, :bottom) | anchor: {0, 0}, goal: nil, last: nil}

  # ---- editing ----

  @doc """
  Inserts `text` at the cursor, in place of the selection if there is one, and leaves the
  cursor after it. A newline in `text` splits the line.
  """
  @spec insert(t(), String.t()) :: t()
  def insert(b, text) do
    typed? = String.length(text) == 1 and text != "\n" and selection(b) == nil

    b
    |> record(if typed?, do: {:type, b.row, b.col}, else: nil)
    |> delete_selection_now()
    |> put(text)
    |> then(&%{&1 | last: if(typed?, do: {:type, &1.row, &1.col}, else: nil)})
  end

  @doc "Deletes the selection, or the grapheme before the cursor, joining lines at a line's start."
  @spec backspace(t()) :: t()
  def backspace(b) do
    cond do
      selection(b) -> b |> record(nil) |> delete_selection_now()
      b.col == 0 and b.row == 0 -> b
      true -> b |> record(nil) |> then(&delete_range(&1, before(&1), {&1.row, &1.col}))
    end
  end

  @doc "Deletes the selection, or the grapheme after the cursor, joining lines at a line's end."
  @spec delete(t()) :: t()
  def delete(b) do
    at_end? = b.col == String.length(b.line)

    cond do
      selection(b) -> b |> record(nil) |> delete_selection_now()
      at_end? and b.below == [] -> b
      true -> b |> record(nil) |> then(&delete_range(&1, {&1.row, &1.col}, after_(&1)))
    end
  end

  defp before(%{col: 0} = b), do: {b.row - 1, b |> to_row(b.row - 1) |> Map.get(:line) |> String.length()}
  defp before(b), do: {b.row, b.col - 1}
  defp after_(b), do: if(b.col == String.length(b.line), do: {b.row + 1, 0}, else: {b.row, b.col + 1})

  @doc "The selected text, or `nil` when nothing is selected."
  @spec selected(t()) :: String.t() | nil
  def selected(b) do
    case selection(b) do
      nil ->
        nil

      {{r1, c1}, {r2, c2}} ->
        lines = slice(b, r1, r2 - r1 + 1)
        last = List.last(lines) |> String.slice(0, c2)
        lines = List.replace_at(lines, -1, last)
        lines |> List.update_at(0, &String.slice(&1, c1..-1//1)) |> Enum.join("\n")
    end
  end

  @doc "Deletes the selection, as one step to undo."
  @spec delete_selection(t()) :: t()
  def delete_selection(b), do: if(selection(b), do: b |> record(nil) |> delete_selection_now(), else: b)

  defp delete_selection_now(b) do
    case selection(b) do
      nil -> b
      {from, to} -> delete_range(b, from, to)
    end
  end

  # Removes the text from `from` to `to`, the cursor at `from`.
  defp delete_range(b, {r1, c1} = _from, {r2, c2}) do
    b = to_row(b, r1)
    last = if r2 == r1, do: b.line, else: Enum.at(b.below, r2 - r1 - 1)
    head = String.slice(b.line, 0, c1)
    tail = String.slice(last, c2..-1//1)
    below = Enum.drop(b.below, r2 - r1)
    %{b | line: head <> tail, below: below, col: c1, size: b.size - (r2 - r1), anchor: nil, goal: nil}
  end

  # Puts `text` at the cursor.
  defp put(b, text) do
    {head, tail} = cut(b.line, b.col)

    case String.split(text, "\n") do
      [one] ->
        %{b | line: head <> one <> tail, col: b.col + String.length(one), goal: nil}

      [first | rest] ->
        {middle, [last]} = Enum.split(rest, -1)
        above = Enum.reverse(middle, [head <> first | b.above])

        %{
          b
          | above: above,
            line: last <> tail,
            row: b.row + length(rest),
            col: String.length(last),
            size: b.size + length(rest),
            goal: nil,
            relinked: b.relinked + length(rest)
        }
    end
  end

  # `line` before grapheme `col` and from it, walking only the graphemes before it, so typing at
  # the start of a long line does not read the whole line.
  defp cut(line, col), do: cut(line, col, 0)
  defp cut(line, 0, at), do: {binary_part(line, 0, at), binary_part(line, at, byte_size(line) - at)}

  defp cut(line, col, at) do
    case String.next_grapheme_size(binary_part(line, at, byte_size(line) - at)) do
      {size, _rest} -> cut(line, col - 1, at + size)
      nil -> {line, ""}
    end
  end

  # Keeps the buffer as it is now as the step undo goes back to, unless `step` continues the
  # last one (typing on at the same place). Every edit gives the text a version never given
  # before (`clock` only grows, undo or not), so `modified?/1` holds after an undo and a new edit.
  defp record(b, step) do
    if step != nil and step == b.last do
      %{b | clock: b.clock + 1, version: b.clock + 1, redo: [], last: nil}
    else
      undo = kept([snapshot(b) | b.undo], 0, 0)
      %{b | clock: b.clock + 1, version: b.clock + 1, undo: undo, redo: [], last: nil, relinked: 0}
    end
  end

  # A step's `relinked` is what it holds that the step before it does not: the lines the cursor
  # crossed or an edit inserted in between, never more than the text's, and the cursor's line,
  # which the next edit copies. Undo keeps the newest step, then older ones while they fit in
  # @depth and @undo_lines.
  defp snapshot(b) do
    relinked = min(b.relinked, b.size) + div(byte_size(b.line), @line_bytes)
    %{b | undo: [], redo: [], last: nil, relinked: relinked}
  end

  defp kept([step | older], n, held) when n == 0 or (n < @depth and held + step.relinked <= @undo_lines),
    do: [step | kept(older, n + 1, held + step.relinked)]

  defp kept(_dropped, _n, _held), do: []

  @doc "Goes back one step, if there is one."
  @spec undo(t()) :: t()
  def undo(%{undo: []} = b), do: b

  def undo(%{undo: [prev | undo]} = b),
    do: %{prev | undo: undo, redo: [snapshot(b) | b.redo], saved: b.saved, clock: b.clock}

  @doc "Goes forward one step undone, if there is one."
  @spec redo(t()) :: t()
  def redo(%{redo: []} = b), do: b

  def redo(%{redo: [next | redo]} = b),
    do: %{next | redo: redo, undo: [snapshot(b) | b.undo], saved: b.saved, clock: b.clock}

  # ---- find and replace ----

  @doc """
  The next match of `pattern` after the cursor (after the selection, if there is one), around to
  the start when none follows; it is selected, the cursor at its end. `:none` when there is no
  match anywhere. A pattern is a `Regex` or a string matched as it is; an empty match is skipped.
  """
  @spec find(t(), Regex.t() | String.t()) :: {:ok, t()} | :none
  def find(b, pattern) do
    {row, col} =
      case selection(b) do
        nil -> {b.row, b.col}
        {_from, to} -> to
      end

    lines = lines(b)
    {before, [here | after_]} = Enum.split(lines, row)

    candidates =
      [{row, here, col}] ++
        Enum.with_index(after_, &{row + 1 + &2, &1, 0}) ++
        Enum.with_index(before, &{&2, &1, 0}) ++ [{row, here, 0}]

    Enum.find_value(candidates, :none, fn {r, line, from} ->
      case match(pattern, line, from) do
        nil -> nil
        {c1, c2} -> {:ok, %{step(b, {:to, r, c2}) | anchor: {r, c1}, goal: nil, last: nil}}
      end
    end)
  end

  # The first non-empty match in `line` starting at grapheme `from`, as grapheme columns.
  defp match(pattern, line, from) do
    offset = line |> String.slice(0, from) |> byte_size()

    found =
      case pattern do
        %Regex{} = re ->
          Regex.scan(re, line, return: :index, offset: offset)
          |> Enum.map(&hd/1)
          |> Enum.find(fn {_at, len} -> len > 0 end)

        text when text != "" ->
          case :binary.match(line, text, scope: {offset, byte_size(line) - offset}) do
            :nomatch -> nil
            at -> at
          end

        _empty ->
          nil
      end

    with {at, len} <- found do
      c1 = String.length(binary_part(line, 0, at))
      {c1, c1 + String.length(binary_part(line, at, len))}
    end
  end

  @doc """
  Replaces every match of `pattern` in every line with `replacement` (a `Regex` replacement,
  `\\\\1` for a group, when the pattern is one), as one step to undo, and says how many lines
  changed.
  """
  @spec replace_all(t(), Regex.t() | String.t(), String.t()) :: {t(), non_neg_integer()}
  def replace_all(b, pattern, replacement) do
    lines = lines(b)
    replaced = Enum.map(lines, &replace_line(&1, pattern, replacement))
    changed = Enum.zip(lines, replaced) |> Enum.count(fn {a, z} -> a != z end)

    if changed == 0 do
      {b, 0}
    else
      text = replaced |> Enum.intersperse("\n") |> IO.iodata_to_binary()
      {row, _col} = cursor(b)
      b = record(b, nil)
      fresh = new(text)

      fresh = %{
        fresh
        | undo: b.undo,
          clock: b.clock,
          version: b.version,
          saved: b.saved,
          relinked: fresh.size
      }

      {step(fresh, {:to, row, 0}), changed}
    end
  end

  @doc """
  Replaces the selection with `replacement` when the selection is a whole match of `pattern`,
  and then finds the next match; otherwise finds the next match without replacing.
  """
  @spec replace_next(t(), Regex.t() | String.t(), String.t()) :: {:ok, t()} | :none
  def replace_next(b, pattern, replacement) do
    b =
      case selected(b) do
        nil ->
          b

        text ->
          if whole_match?(pattern, text),
            do: insert(%{b | last: nil}, replace_line(text, pattern, replacement)),
            else: b
      end

    find(b, pattern)
  end

  defp whole_match?(%Regex{} = re, text) do
    case Regex.run(re, text, return: :index) do
      [{0, len} | _] -> len == byte_size(text) and len > 0
      _ -> false
    end
  end

  defp whole_match?(pattern, text), do: pattern != "" and pattern == text

  defp replace_line(line, %Regex{} = re, replacement), do: Regex.replace(re, line, replacement)
  defp replace_line(line, "", _replacement), do: line
  defp replace_line(line, text, replacement), do: String.replace(line, text, replacement)
end
