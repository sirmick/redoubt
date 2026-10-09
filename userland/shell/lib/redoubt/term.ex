defmodule Redoubt.Term do
  @moduledoc """
  The terminal library's encoder: the one writer of control sequences (docs/userland/shell.md,
  "The terminal library").

  It draws what OTP's `group` asks of a terminal driver, a line being edited and text printed
  while it is, and keeps a model of that line: the text before the cursor and after it, the
  prompt at its head, wrapped at the terminal's width. Every grapheme drawn passes
  `Redoubt.Term.Text`'s rule, so a control character in a prompt, in typed text or in printed
  output is drawn visibly and never reaches the terminal as itself. The one exception is
  `group`'s own search prompt, which it hardcodes with its bold sequences: it is matched exactly,
  at the head of what `group` inserts for it, and drawn in bold; anything else that looks like
  it is drawn as text. Lines end in CR LF, since Redoubt's console does no output processing.

  A request is drawn by redrawing the line from its start: the cursor goes back to where the
  line began, the screen is erased from there, the line is drawn again and the cursor is placed.
  That costs a line's length per key and asks nothing of the terminal but VT102: relative cursor
  movement, CR, LF, erasing to the end of the screen, and the bold and underline attributes. A
  line taller than the screen is drawn, but its start has scrolled off and its editing is not
  exact. Printed text is made visible and measured a piece at a time, so drawing a line needs
  memory beyond the line only for a piece. The line itself is not bounded: `group` delivers it
  whole, so one line larger than the driver's heap limit (about 2.75 MiB on rv64) still ends the
  session.
  """

  alias Redoubt.Term.{Text, Width}

  # group.erl's own prompt for Ctrl+R, the one sequence it writes itself (docs/userland/shell.md,
  # "Line editing and history"); drawn as `search: ` in bold and underline.
  @search_prompt "\e[;1;4msearch:\e[0m "
  @search_text "search: "
  @styled String.length("search:")

  @csi "\e["

  # The most bytes of printed text made visible and measured at once (`draw_text/3`).
  @piece 4096

  defstruct cols: 80,
            rows: 24,
            # Graphemes before the cursor, the last first, and after it, in order. A newline is
            # the grapheme "\n".
            before: [],
            after: [],
            # How many graphemes at the head of the line are the prompt, and how many of those
            # are drawn styled (the search prompt).
            prompt: 0,
            styled: 0,
            # Whether the next insertion is a prompt: set by new_prompt, since edlin inserts the
            # prompt as it inserts anything else.
            fresh: true,
            # The column the line begins at: not 0 after text printed without a final newline.
            origin: 0,
            # The code points printed since the last newline, so a tab printed next stops where
            # it would in the whole line, however the line was printed.
            tab: 0,
            # Text shown below the line (completions, help), its first shown row and its row limit.
            expand: nil,
            expand_row: 1,
            expand_limit: 0,
            # Where the cursor is, raw, kept by a key typed at the line's end for the next request
            # (`request/2`): a key typed there costs the key, not the line (`cursor/1`).
            at: nil

  @type t :: %__MODULE__{}

  @doc "An encoder for a terminal of `cols` by `rows`, with nothing drawn."
  @spec new(pos_integer(), pos_integer()) :: t()
  def new(cols, rows), do: %__MODULE__{cols: max(cols, 1), rows: max(rows, 1)}

  @doc "The terminal's size now; the line is laid out at the new width from its next redraw."
  @spec resize(t(), pos_integer(), pos_integer()) :: t()
  def resize(term, cols, rows), do: %{term | cols: max(cols, 1), rows: max(rows, 1), at: nil}

  @doc """
  `request` as the requests to draw one after another, so that no drawing is held whole: text
  printed with no line open, past `size` bytes, as pieces of it of at most `size` bytes, each cut
  where a character starts, which draw exactly as the whole would; any other request as itself.
  A driver writes each one's bytes before it draws the next, since the VM counts the binaries a
  process holds toward its heap limit.
  """
  @spec slices(t(), term(), pos_integer()) :: Enumerable.t()
  def slices(%__MODULE__{before: [], after: []}, request, size) do
    case printed(request) do
      {:ok, text} when byte_size(text) > size ->
        text
        |> Stream.unfold(fn
          <<>> -> nil
          text -> cut(text, size)
        end)
        |> Stream.map(&{:put_chars, :unicode, &1})

      _short_or_none ->
        [request]
    end
  end

  def slices(_term, request, _size), do: [request]

  defp printed({:put_chars_sync, encoding, chars, _reply}), do: {:ok, text(chars, encoding)}
  defp printed({:put_chars, encoding, chars}), do: {:ok, text(chars, encoding)}
  defp printed(_request), do: :error

  @doc "Whether a line is being edited and holds nothing but its prompt."
  @spec line_empty?(t()) :: boolean()
  def line_empty?(%__MODULE__{fresh: false, before: before, after: [], prompt: prompt}),
    do: length(before) == prompt

  def line_empty?(_term), do: false

  @doc "Whether a line is being edited: a prompt has been drawn and no line has ended since."
  @spec line_open?(t()) :: boolean()
  def line_open?(%__MODULE__{fresh: fresh}), do: not fresh

  @doc """
  Draws the interrupt: `^C` after the line, a new line, and no line open. The driver then tells
  `group`, which drops the line.
  """
  @spec interrupt(t()) :: {iodata(), t()}
  def interrupt(term) do
    to_end = move(norm(cursor(term), term.cols), norm(line_end(term), term.cols))
    {[to_end, "^C\r\n"], reset(term)}
  end

  @doc """
  Draws one of `group`'s requests to its driver, and returns the bytes to write and the model
  after it. A request this encoder does not draw (`open_editor`, say) draws nothing.
  """
  @spec request(t(), term()) :: {iodata(), t()}
  def request(term, request) do
    {out, model} = handle(term, request)
    {out, %{model | at: kept(model.at, term.at)}}
  end

  # The cursor a request typed at the line's end is kept for the next; one kept by a request
  # within this one stays; one this request did not touch goes, since the line may have changed.
  defp kept({:typed, pos}, _entry), do: {:kept, pos}
  defp kept(at, entry) when at == entry, do: nil
  defp kept(at, _entry), do: at

  defp handle(%__MODULE__{expand: expand} = term, request)
       when expand != nil and
              not (is_tuple(request) and elem(request, 0) in [:move_expand, :put_expand, :requests]) do
    # Anything but paging the text below the line takes it away first, as prim_tty does.
    {cleared, term} = repaint(term, %{term | expand: nil, expand_row: 1, expand_limit: 0})
    {drawn, term} = request(term, request)
    {[cleared, drawn], term}
  end

  defp handle(term, {:requests, requests}) do
    Enum.reduce(requests, {[], term}, fn request, {out, term} ->
      {drawn, term} = request(term, request)
      {[out, drawn], term}
    end)
  end

  defp handle(term, {:put_chars_sync, encoding, chars, _reply}), do: put_chars(term, text(chars, encoding))
  defp handle(term, {:put_chars, encoding, chars}), do: put_chars(term, text(chars, encoding))
  defp handle(term, {:insert_chars, encoding, chars}), do: insert(term, text(chars, encoding), false)
  defp handle(term, {:insert_chars_over, encoding, chars}), do: insert(term, text(chars, encoding), true)
  defp handle(term, {:delete_chars, 0}), do: {[], term}

  defp handle(term, {:delete_chars, n}) when n > 0,
    do: repaint(term, %{term | after: Enum.drop(term.after, n)})

  defp handle(term, {:delete_chars, n}) when n < 0 do
    # Never into the prompt, which edlin never asks; the cap keeps a wrong count from it.
    kept = max(length(term.before) + n, term.prompt)
    repaint(term, %{term | before: Enum.take(term.before, -kept)})
  end

  defp handle(term, {:move_rel, 0}), do: {[], term}

  defp handle(term, {:move_rel, n}) when n < 0 do
    {moved, before} = Enum.split(term.before, -n)
    repaint(term, %{term | before: before, after: Enum.reverse(moved, term.after)})
  end

  defp handle(term, {:move_rel, n}) when n > 0 do
    {moved, rest} = Enum.split(term.after, n)
    repaint(term, %{term | before: Enum.reverse(moved, term.before), after: rest})
  end

  defp handle(term, {:move_line, 0}), do: {[], term}
  defp handle(term, {:move_line, n}), do: repaint(term, move_line(term, n))

  defp handle(term, {:move_combo, before, lines, after_}) do
    request(term, {:requests, [{:move_rel, before}, {:move_line, lines}, {:move_rel, after_}]})
  end

  defp handle(term, {:put_expand, encoding, chars, limit}) do
    repaint(term, %{term | expand: text(chars, encoding), expand_row: 1, expand_limit: limit})
  end

  defp handle(%{expand: nil} = term, {:move_expand, _n}), do: {[], term}

  defp handle(term, {:move_expand, n}) do
    shown = expand_rows(term, term |> line_end() |> norm(term.cols) |> elem(0))
    last_first = max(length(expand_lines(term)) - shown + 1, 1)
    repaint(term, %{term | expand_row: term.expand_row |> Kernel.+(n) |> max(1) |> min(last_first)})
  end

  defp handle(term, {:redraw_prompt, prompt, continuation, {lines_before, {before, after_}, lines_after}}) do
    continuation = graphemes(text(continuation, :unicode))
    line = fn chars -> graphemes(text(chars, :unicode)) end
    earlier = lines_before |> Enum.reverse() |> Enum.flat_map(&(line.(&1) ++ ["\n" | continuation]))
    later = Enum.flat_map(lines_after, &["\n" | continuation ++ line.(&1)])
    current_before = before |> Enum.reverse() |> line.()

    repaint(
      term,
      with_prompt(term, text(prompt, :unicode), earlier ++ current_before, line.(after_) ++ later)
    )
  end

  defp handle(term, :redraw_prompt), do: repaint(term, term)

  defp handle(term, :new_prompt) do
    # The line is over, and the next begins where the cursor is, which the ended line's end
    # settles: after its newline, column 0.
    origin = norm(line_end(term), term.cols) |> elem(1)
    {[], %{reset(term) | origin: origin}}
  end

  defp handle(term, :delete_line),
    do: {[to_origin(term), erase_below()], %{reset(term) | origin: term.origin}}

  defp handle(term, :delete_after_cursor), do: {erase_below(), %{term | after: []}}
  defp handle(term, :beep), do: {"\a", term}
  defp handle(term, :clear), do: {[@csi, "H", @csi, "2J"], %{reset(term) | origin: 0}}
  defp handle(term, _other), do: {[], term}

  # ---- the requests' work ----

  # Text printed by the shell or anything else on the group: above the line being edited, if
  # one is, which is drawn again below it.
  defp put_chars(%{before: [], after: []} = term, text) do
    {drawn, end_, tab} = draw_text(text, term.tab, {0, term.origin}, term.cols)
    {_row, origin} = norm(end_, term.cols)
    {[drawn, margin(end_, term.cols)], %{term | origin: origin, tab: tab}}
  end

  defp put_chars(term, text) do
    text = if String.ends_with?(text, "\n"), do: text, else: text <> "\n"
    {drawn, {_row, col}, tab} = draw_text(text, term.tab, {0, term.origin}, term.cols)
    moved = %{term | origin: col, tab: tab}
    {[to_origin(term), erase_below(), drawn, draw(moved)], moved}
  end

  defp insert(%{fresh: true} = term, text, _over), do: repaint(term, with_prompt(term, text, [], term.after))

  defp insert(term, text, over) do
    inserted = graphemes(text)
    after_ = if over, do: Enum.drop(term.after, length(inserted)), else: term.after
    model = %{term | before: Enum.reverse(inserted, term.before), after: after_}

    if term.after == [] and term.expand == nil and "\n" not in inserted do
      # Typing at the end of the line: the characters alone, as a terminal echoes, and the cursor
      # kept where they end.
      {drawn, end_} = draw_graphemes(inserted, cursor(term), term.cols, 0)
      {[drawn, margin(end_, term.cols)], %{model | at: {:typed, end_}}}
    else
      repaint(term, model)
    end
  end

  # Moving `n` lines up or down within a line that holds newlines, keeping the column where
  # the target line is long enough.
  defp move_line(term, n) do
    {lines, row, col} = lines(term)
    target = row + n

    if target < 0 or target >= length(lines) do
      term
    else
      line = Enum.at(lines, target)
      col = min(col, length(line))
      {this_before, this_after} = Enum.split(line, col)
      earlier = lines |> Enum.take(target) |> Enum.flat_map(&(&1 ++ ["\n"]))
      later = lines |> Enum.drop(target + 1) |> Enum.flat_map(&["\n" | &1])
      %{term | before: Enum.reverse(earlier ++ this_before), after: this_after ++ later}
    end
  end

  # The line as its lines, with the cursor's line and column within it.
  defp lines(term) do
    row = Enum.count(term.before, &(&1 == "\n"))
    col = term.before |> Enum.take_while(&(&1 != "\n")) |> length()
    all = Enum.reverse(term.before, term.after)
    {Enum.chunk_while(all, [], &line_chunk/2, &{:cont, Enum.reverse(&1), []}), row, col}
  end

  defp line_chunk("\n", acc), do: {:cont, Enum.reverse(acc), []}
  defp line_chunk(g, acc), do: {:cont, [g | acc]}

  # The prompt's graphemes, how many of them are styled, and the text group inserted with the
  # prompt in one request (the search prompt comes joined with what is searched for): group's
  # search prompt, matched exactly at the head, is drawn in bold; any other sequence in a
  # prompt is text.
  defp prompt(@search_prompt <> typed), do: {graphemes(@search_text), @styled, graphemes(typed)}
  defp prompt(text), do: {graphemes(text), 0, []}

  # The line opened by the prompt `text`, with the graphemes `before` the cursor after it and
  # `after_` the cursor.
  defp with_prompt(term, text, before, after_) do
    {prompt, styled, typed} = prompt(text)
    lead = prompt ++ typed

    %{
      term
      | before: Enum.reverse(lead ++ before),
        after: after_,
        prompt: length(prompt),
        styled: styled,
        fresh: false
    }
  end

  defp reset(term), do: %__MODULE__{cols: term.cols, rows: term.rows}

  # ---- drawing ----

  # From the cursor at `old`'s position: to the line's start, erase, draw `new` and place its
  # cursor.
  defp repaint(old, new), do: {[to_origin(old), erase_below(), draw(new)], new}

  # The line, the text below it, and the cursor placed, from the line's origin: the line drawn in
  # one pass, which finds the cursor on its way.
  defp draw(term) do
    before = Enum.reverse(term.before)
    {head, cursor} = draw_graphemes(before, {0, term.origin}, term.cols, term.styled)
    {tail, line_end} = draw_graphemes(term.after, cursor, term.cols, max(term.styled - length(before), 0))
    {below, end_} = draw_expand(term, norm(line_end, term.cols))
    cursor = norm(cursor, term.cols)

    [
      head,
      tail,
      margin(line_end, term.cols),
      below,
      margin(end_, term.cols),
      move(norm(end_, term.cols), cursor)
    ]
  end

  # Graphemes from `pos`, the first `styled` of them in bold and underline: the bytes and the
  # raw end position (a column equal to the width is the margin, pending a wrap). A redraw takes
  # every grapheme of the line, so each is a few steps: on the machine a step costs about 87 µs.
  defp draw_graphemes(graphemes, pos, cols, styled), do: draw_graphemes(graphemes, pos, cols, styled, [])

  defp draw_graphemes([], pos, _cols, _styled, out), do: {out, pos}

  # A printable ASCII byte that fits on the row, most of any line: one step.
  defp draw_graphemes([<<c>> = g | rest], {row, col}, cols, 0, out) when c in 0x20..0x7E and col < cols,
    do: draw_graphemes(rest, {row, col + 1}, cols, 0, [out | g])

  defp draw_graphemes([g | rest], pos, cols, 0, out),
    do: draw_graphemes(rest, advance(pos, g, cols), cols, 0, [out | visible(g)])

  defp draw_graphemes([g | rest], pos, cols, styled, out) do
    drawn = [@csi, "1;4m", visible(g), @csi, "0m"]
    draw_graphemes(rest, advance(pos, g, cols), cols, styled - 1, [out | drawn])
  end

  # Printed text: visible, with CR LF for each newline, a piece at a time, each at most `@piece`
  # bytes and cut before a newline or where a character starts, its tab stops counted on from
  # `tab`, the code points its line holds already. So a line of anything is made visible and
  # measured in memory beyond it bounded by the piece, not the line (one process's heap is a
  # sixteenth of a session's budget). A grapheme cut between two pieces is measured as its parts.
  # The drawing, where it ends, and the code points its last line holds.
  defp draw_text(text, tab, pos, cols), do: draw_text(text, tab, pos, cols, [])

  defp draw_text(<<>>, tab, pos, _cols, out), do: {out, pos, tab}

  defp draw_text(<<?\n, rest::binary>>, _tab, {row, _col}, cols, out),
    do: draw_text(rest, 0, {row + 1, 0}, cols, [out, "\r\n"])

  defp draw_text(text, tab, pos, cols, out) do
    {piece, rest} = piece(text)
    {visible, tab} = Text.visible(piece, tab)
    draw_text(rest, tab, advance_text(pos, visible, cols), cols, [out, visible])
  end

  # The text up to its first newline, at most `@piece` bytes of it, cut where a character starts.
  defp piece(text) do
    case :binary.match(text, "\n", scope: {0, min(byte_size(text), @piece)}) do
      {at, 1} -> split(text, at)
      :nomatch -> cut(text, @piece)
    end
  end

  # The text cut at most `size` bytes in, where a character starts: the head and the rest.
  defp cut(text, size) do
    size = min(byte_size(text), size)
    split(text, char_start(text, size, size))
  end

  defp split(text, at) do
    <<head::binary-size(^at), rest::binary>> = text
    {head, rest}
  end

  # `size`, moved back to the start of the UTF-8 sequence the cut falls in; `size` itself if the
  # cut is at the end, or the bytes there are no sequence, which are drawn as bytes anyway.
  defp char_start(text, at, size) when at > 0 and at < byte_size(text) and size - at <= 3 do
    case :binary.at(text, at) do
      b when b in 0x80..0xBF -> char_start(text, at - 1, size)
      _start -> at
    end
  end

  defp char_start(_text, _at, size), do: size

  # Where text already made visible ends, wrapped at the width, a grapheme at a time, each
  # measured as it is, being visible already. A run of code points each a grapheme one column
  # wide (`Width.run/1`) is taken at once, but for its last when more follows: a combining mark
  # after it would join it. A run starts only on a byte of printable ASCII or the lead byte of a
  # code point below U+0540, so any other byte starts a grapheme taken alone, without asking.
  defp advance_text(pos, <<b, _::binary>> = text, cols) when b in 0x20..0x7E or b in 0xC2..0xD4 do
    case Width.run(text) do
      {bytes, points} when bytes == byte_size(text) ->
        advance_columns(pos, points, cols)

      {bytes, points} when points > 1 ->
        run = bytes - if :binary.at(text, bytes - 1) < 0x80, do: 1, else: 2
        <<_::binary-size(^run), rest::binary>> = text
        advance_text(advance_columns(pos, points - 1, cols), rest, cols)

      _one_or_none ->
        advance_grapheme(pos, text, cols)
    end
  end

  defp advance_text(pos, text, cols), do: advance_grapheme(pos, text, cols)

  defp advance_grapheme(pos, text, cols) do
    case String.next_grapheme(text) do
      {g, rest} -> advance_text(step(pos, Width.grapheme(g), cols), rest, cols)
      nil -> pos
    end
  end

  # `step/3` for `n` graphemes each one column wide: those that fit on the row, then whole rows.
  defp advance_columns(pos, 0, _cols), do: pos

  defp advance_columns({row, col}, n, cols) do
    case n - max(cols - col, 0) do
      left when left <= 0 -> {row, col + n}
      left -> {row + div(left - 1, cols) + 1, rem(left - 1, cols) + 1}
    end
  end

  # The text below the line: at most the rows the limit and the screen allow, from the row
  # paged to, with a line saying so when there is more.
  defp draw_expand(%{expand: nil}, end_), do: {[], end_}

  defp draw_expand(term, {row, _col}) do
    lines = expand_lines(term)
    shown = expand_rows(term, row)
    page = lines |> Enum.drop(term.expand_row - 1) |> Enum.take(shown)

    page =
      if length(lines) > shown do
        page ++ ["rows #{term.expand_row} to #{term.expand_row + length(page) - 1} of #{length(lines)}"]
      else
        page
      end

    # Each row below the line starts a row of its own; the last leaves the cursor at its end.
    Enum.reduce(page, {[], {row, 0}}, fn line, {out, {row, _col}} ->
      {drawn, pos, _tab} = draw_text(line, 0, {row + 1, 0}, term.cols)
      {[out, "\r\n", drawn], pos}
    end)
  end

  defp expand_lines(%{expand: expand}), do: String.split(expand, "\n")

  # Rows of text below the line the limit and the screen allow: the screen less the line's rows,
  # to `row`, where it ends, and one for the status line.
  defp expand_rows(term, row) do
    free = max(term.rows - row - 2, 1)
    if term.expand_limit > 0, do: min(term.expand_limit, free), else: free
  end

  # ---- positions ----

  # Where the cursor is, from the line's origin, raw: as kept by the key typed last, else found by
  # walking the line from its start. Only the model a request was given has a kept cursor that is
  # its own, so this is asked of that model alone: a model a request makes is drawn in one pass
  # that finds its cursor and its end (`draw/1`).
  defp cursor(%{at: {:kept, pos}}), do: pos
  defp cursor(term), do: term.before |> Enum.reverse() |> advance_all({0, term.origin}, term.cols)

  # Where the line ends, raw.
  defp line_end(term), do: advance_all(term.after, cursor(term), term.cols)

  defp advance_all([], pos, _cols), do: pos

  defp advance_all([<<c>> | rest], {row, col}, cols) when c in 0x20..0x7E and col < cols,
    do: advance_all(rest, {row, col + 1}, cols)

  defp advance_all([g | rest], pos, cols), do: advance_all(rest, advance(pos, g, cols), cols)

  defp advance({row, _col}, "\n", _cols), do: {row + 1, 0}
  defp advance(pos, g, cols), do: step(pos, width(g), cols)

  # A grapheme `w` columns wide drawn at `pos`: on the row if it fits, else at the next row's start.
  defp step({row, col}, w, cols) when col + w > cols, do: {row + 1, w}
  defp step({row, col}, w, _cols), do: {row, col + w}

  # A position at the margin, pending a wrap, is the next row's start.
  defp norm({row, col}, cols) when col >= cols, do: {row + 1, 0}
  defp norm(pos, _cols), do: pos

  # After drawing to the margin the terminal holds the cursor there, pending; a space and a
  # backspace settle it at the next row's start, where the model has it.
  defp margin({_row, col}, cols) when col >= cols, do: " \b"
  defp margin(_pos, _cols), do: []

  # A grapheme of one printable ASCII byte, most of any line, is itself and one column wide.
  defp width(<<c>>) when c in 0x20..0x7E, do: 1
  defp width(g), do: Width.columns(visible(g))

  defp visible("\n"), do: "\r\n"
  defp visible(<<c>> = g) when c in 0x20..0x7E, do: g
  defp visible(g), do: Text.visible(g)

  # ---- the sequences ----

  defp to_origin(term), do: move(norm(cursor(term), term.cols), {0, term.origin})

  defp move({from_row, _from_col}, {to_row, to_col}), do: [vertical(from_row - to_row), "\r", right(to_col)]

  # `rows` up, or as many down when it is negative.
  defp vertical(0), do: []
  defp vertical(rows) when rows > 0, do: [@csi, Integer.to_string(rows), "A"]
  defp vertical(rows), do: [@csi, Integer.to_string(-rows), "B"]

  defp right(0), do: []
  defp right(n), do: [@csi, Integer.to_string(n), "C"]

  defp erase_below, do: [@csi, "J"]

  # ---- text ----

  # A grapheme list, with each newline its own grapheme (never joined with a CR before it).
  defp graphemes(text),
    do:
      text
      |> String.split("\n")
      |> Enum.map(&String.graphemes/1)
      |> Enum.intersperse(["\n"])
      |> List.flatten()

  # group's characters as a string: unicode, or latin1 when it says so. A binary in unicode is
  # the string already, valid or not (the bytes that are not UTF-8 are drawn as bytes): not
  # copied, which for a long line would double what the driver holds.
  defp text(chars, encoding) when is_binary(chars) and encoding != :latin1, do: chars

  defp text(chars, encoding) do
    case :unicode.characters_to_binary(chars, if(encoding == :latin1, do: :latin1, else: :unicode)) do
      binary when is_binary(binary) -> binary
      _error -> IO.iodata_to_binary(chars)
    end
  end
end
