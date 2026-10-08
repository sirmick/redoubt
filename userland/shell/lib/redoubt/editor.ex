defmodule Redoubt.Editor do
  @moduledoc """
  The editor (docs/userland/shell.md, "The editor"): `ed(path)`, a screen program in the session's
  VM. Its text is `Redoubt.Editor.Buffer`, what it does to files is `Redoubt.Editor.Files`, each
  line is drawn as `Redoubt.Editor.View` lays it out, in the roles `Redoubt.Editor.Syntax` gives
  its parts when the file's extension names a language.

  The keys are micro's where a terminal lets them be: Ctrl+S saves, Ctrl+Q closes, Ctrl+F finds
  and Ctrl+N finds the next, Ctrl+R replaces, Ctrl+L goes to a line, Ctrl+Z undoes and Ctrl+Y
  redoes, Ctrl+X cuts, Ctrl+C copies and Ctrl+V pastes, Ctrl+A selects all; Shift with a cursor
  key selects. Ctrl+O opens another file, and Alt+, and Alt+. go to the one before and after. F10,
  or Alt and a menu's first letter, opens the menu bar. The editor takes Ctrl+C as a key; the
  session's own key, Ctrl+\\, ends it, unsaved changes and all, as it ends any screen.

  Nothing a key does reaches a file without the person: a save, a close, or an open that arrives
  in a burst of keys asks first, and the keys that arrive with that question are dropped, so a
  pasted Enter cannot answer it. A key is in a burst when more keys are already waiting behind it
  (a paste, which the terminal sends all at once), or when it comes within 300 ms of one that
  had: so a paste that ends in Ctrl+S, with nothing behind its last key, asks too.
  """

  use Redoubt.Commandlet, area: "Screens"

  @behaviour Redoubt.Screen

  alias Redoubt.Editor.{Buffer, Files, Syntax, View}
  alias Redoubt.Screen.{Layout, Widgets}
  alias Redoubt.Screen.Widget.{Dialogs, MenuBar, Theme}
  alias Redoubt.Term.Buffer, as: Cells
  alias Redoubt.Term.Text

  # Keys arriving this long after a question a burst of keys raised are dropped with it.
  @deaf_ms 300

  # A key this soon after one that had more keys waiting behind it is of the same burst: the last
  # key of a paste meets an empty queue. A person's next key comes later than this.
  @burst_ms 300

  # Lines between the highlighting's start states that a file keeps: a window is highlighted
  # from the one at or above its top.
  @every 128

  @hint "^S save · ^Q close · ^F find · ^C copy · ^V paste · ^Z undo · F10 menu"

  @menus [
    {"File",
     [
       {"Open...  Ctrl+O", :open},
       {"Save  Ctrl+S", :save},
       {"Close  Ctrl+Q", :close},
       :separator,
       {"Previous file  Alt+,", :previous},
       {"Next file  Alt+.", :next}
     ]},
    {"Edit",
     [
       {"Undo  Ctrl+Z", :undo},
       {"Redo  Ctrl+Y", :redo},
       :separator,
       {"Cut  Ctrl+X", :cut},
       {"Copy  Ctrl+C", :copy},
       {"Paste  Ctrl+V", :paste},
       {"Select all  Ctrl+A", :select_all}
     ]},
    {"Search",
     [
       {"Find...  Ctrl+F", :find},
       {"Find next  Ctrl+N", :find_next},
       {"Replace...  Ctrl+R", :replace},
       {"Go to line...  Ctrl+L", :go_to}
     ]}
  ]

  @summary "Edit a file, on a screen"
  @help """
  Opens path in the editor, a screen of its own; a path that is not there is a new file, made
  when it is first saved. The keys: Ctrl+S save, Ctrl+Q close, Ctrl+F find, Ctrl+N find next,
  Ctrl+R replace (a pattern between slashes, /like this/, is a regular expression), Ctrl+L go to
  a line, Ctrl+Z undo, Ctrl+Y redo, Ctrl+X cut, Ctrl+C copy, Ctrl+V paste, Ctrl+A select all,
  Shift with a cursor key to select, Ctrl+O open another file, Alt+, and Alt+. to go between
  them, F10 for the menus. Ctrl+\\, the session's own key, ends the editor, unsaved changes and
  all.

  A file that is not UTF-8 opens read only, every byte that is not text shown as <FF>. A file
  larger than the editor holds is refused, with its size and the limit. A save goes to the path
  opened and nowhere else, through a new file and a rename, and asks before overwriting a file
  that changed on disk since it was read.
  """
  @args path: "the file to edit"
  @examples [{~S'ed("notes.txt")', "edit notes.txt"}]
  defcommand ed(path :: path) do
    case open(path) do
      {:ok, doc} -> Redoubt.Screen.run(__MODULE__, doc, ctrl_c: :key)
      {:error, reason} -> {:error, reason}
    end
  end

  # ---- files ----

  @doc false
  # A file opened for editing: what is read, or a new file where nothing is. With `view: true`,
  # read only, and a file that is not there is refused.
  @spec open(Path.t(), keyword()) :: {:ok, map()} | {:error, term()}
  def open(path, opts \\ []) do
    path = Path.expand(path)
    view = Keyword.get(opts, :view, false)

    case Files.read(path) do
      {:ok, %{bytes: bytes, utf8: true, digest: digest}} ->
        {:ok, doc(path, Buffer.new(bytes), digest, if(view, do: :viewing, else: false))}

      {:ok, %{bytes: bytes, utf8: false, digest: digest}} ->
        {:ok, doc(path, Buffer.new(shown(bytes)), digest, :not_utf8)}

      {:error, :enoent} when not view ->
        {:ok, doc(path, Buffer.new(), :absent, false)}

      {:error, _reason} = error ->
        error
    end
  end

  defp doc(path, buffer, digest, readonly) do
    %{
      path: path,
      buffer: buffer,
      digest: digest,
      readonly: readonly,
      top: 0,
      left: 0,
      lang: Syntax.language(path),
      marks: %{0 => Syntax.start()},
      seen: {buffer.version, 0}
    }
  end

  # Bytes that are not all UTF-8, as text: each byte outside it as <FF>.
  defp shown(bytes) do
    bytes
    |> String.chunk(:valid)
    |> Enum.map_join(fn chunk ->
      if String.valid?(chunk),
        do: chunk,
        else: for(<<b <- chunk>>, into: "", do: "<" <> Base.encode16(<<b>>) <> ">")
    end)
  end

  # ---- the screen ----

  @impl Redoubt.Screen
  def init(doc) do
    %{
      docs: [doc],
      at: 0,
      bar: MenuBar.new(@menus),
      dialogs: Dialogs.new(),
      theme: Theme.get(:plain),
      clipboard: nil,
      pattern: nil,
      replacement: nil,
      message: nil,
      size: {80, 24},
      deaf_until: nil,
      burst_at: nil
    }
  end

  @impl Redoubt.Screen
  def update(event, state) do
    case handle(event, state) do
      {:cont, state} -> {:cont, highlighted(state)}
      {:halt, _value} = halt -> halt
    end
  end

  defp handle({:resize, cols, rows}, state), do: {:cont, %{state | size: {cols, rows}} |> follow()}

  defp handle({:key, _, _} = key, state) do
    now = System.monotonic_time(:millisecond)
    queued = queued?()
    burst = queued or (state.burst_at != nil and now - state.burst_at < @burst_ms)
    state = if queued, do: %{state | burst_at: now}, else: state
    if deaf?(state, now), do: {:cont, state}, else: key(state, key, burst)
  end

  defp handle(_message, state), do: {:cont, state}

  # More keys waiting behind this one: what a paste is, since the terminal sends it all at once.
  defp queued? do
    {:message_queue_len, n} = Process.info(self(), :message_queue_len)
    n > 0
  end

  defp deaf?(%{deaf_until: nil}, _now), do: false
  defp deaf?(state, now), do: now < state.deaf_until

  defp key(state, key, burst) do
    cond do
      Dialogs.open?(state.dialogs) -> dialog_key(state, key)
      MenuBar.open?(state.bar) -> menu_key(state, key, burst)
      true -> editor_key(%{state | message: nil}, key, burst)
    end
  end

  defp menu_key(state, key, burst) do
    case MenuBar.key(state.bar, key) do
      {:cont, bar} -> {:cont, %{state | bar: bar}}
      {:done, command, bar} -> command(%{state | bar: bar}, command, burst)
      :pass -> {:cont, state}
    end
  end

  defp dialog_key(state, key) do
    case Dialogs.key(state.dialogs, key) do
      {:cont, dialogs} -> {:cont, %{state | dialogs: dialogs}}
      {:closed, id, value, dialogs} -> answered(%{state | dialogs: dialogs}, id, value)
      :pass -> {:cont, state}
    end
  end

  @commands %{
    {"s", [:ctrl]} => :save,
    {"q", [:ctrl]} => :close,
    {"o", [:ctrl]} => :open,
    {"f", [:ctrl]} => :find,
    {"n", [:ctrl]} => :find_next,
    {"r", [:ctrl]} => :replace,
    {"l", [:ctrl]} => :go_to,
    {"z", [:ctrl]} => :undo,
    {"y", [:ctrl]} => :redo,
    {"x", [:ctrl]} => :cut,
    {"c", [:ctrl]} => :copy,
    {"v", [:ctrl]} => :paste,
    {"a", [:ctrl]} => :select_all,
    {",", [:alt]} => :previous,
    {".", [:alt]} => :next
  }

  @moves %{
    left: :left,
    right: :right,
    up: :up,
    down: :down,
    home: :home,
    end: :end
  }

  defp editor_key(state, {:key, key, mods} = k, burst) do
    case MenuBar.key(state.bar, k) do
      {:cont, bar} ->
        {:cont, %{state | bar: bar}}

      _closed_or_pass ->
        case Map.fetch(@commands, {key, mods}) do
          {:ok, command} -> command(state, command, burst)
          :error -> {:cont, edit_key(state, key, mods) |> follow()}
        end
    end
  end

  defp edit_key(state, key, mods) do
    select = :shift in mods
    plain = mods -- [:shift]
    {_cols, rows} = state.size

    cond do
      Map.has_key?(@moves, key) and plain == [] ->
        buffer(state, &Buffer.move(&1, @moves[key], select: select))

      key in [:left, :right] and plain == [:ctrl] ->
        buffer(state, &Buffer.move(&1, if(key == :left, do: :word_left, else: :word_right), select: select))

      key in [:home, :end] and plain == [:ctrl] ->
        buffer(state, &Buffer.move(&1, if(key == :home, do: :top, else: :bottom), select: select))

      key in [:page_up, :page_down] and plain == [] ->
        page = max(rows - 3, 1)
        buffer(state, &Buffer.move(&1, {:page, if(key == :page_up, do: -page, else: page)}, select: select))

      key == :enter and mods == [] ->
        change(state, &Buffer.insert(&1, "\n"))

      key == :tab and mods == [] ->
        change(state, &Buffer.insert(&1, "\t"))

      key == :backspace and mods == [] ->
        change(state, &Buffer.backspace/1)

      key == :delete and mods == [] ->
        change(state, &Buffer.delete/1)

      is_binary(key) and mods in [[], [:shift]] ->
        change(state, &Buffer.insert(&1, key))

      true ->
        state
    end
  end

  # ---- commands ----

  # What reaches a file, or ends the editor, asks first when it came in a burst of keys.
  @guarded [:save, :close, :open]

  defp command(state, command, true) when command in @guarded do
    text = "Keys arriving all at once, as a paste does, asked to #{command}. Do it?"
    {:cont, state |> ask(Dialogs.confirm({:guard, command}, "Pasted keys", text)) |> deafen()}
  end

  defp command(state, :save, _burst), do: save(state, current(state).digest)
  defp command(state, :close, _burst), do: close(state)

  defp command(state, :open, _burst),
    do: {:cont, ask(state, Dialogs.prompt(:open, "Open", "The file to open:", ""))}

  defp command(state, :find, _burst),
    do:
      {:cont,
       ask(state, Dialogs.prompt(:find, "Find", "Text, or /a regular expression/:", source(state.pattern)))}

  defp command(%{pattern: nil} = state, :find_next, burst), do: command(state, :find, burst)
  defp command(state, :find_next, _burst), do: {:cont, find(state) |> follow()}

  defp command(state, :replace, _burst),
    do:
      {:cont,
       ask(state, Dialogs.prompt(:replace, "Replace", "Replace every match of:", source(state.pattern)))}

  defp command(state, :go_to, _burst), do: {:cont, ask(state, Dialogs.prompt(:go_to, "Go to", "Line:", ""))}
  defp command(state, :undo, _burst), do: {:cont, change(state, &Buffer.undo/1) |> unmarked() |> follow()}
  defp command(state, :redo, _burst), do: {:cont, change(state, &Buffer.redo/1) |> unmarked() |> follow()}

  defp command(state, :copy, _burst) do
    case Buffer.selected(current(state).buffer) do
      nil -> {:cont, %{state | message: "nothing selected"}}
      text -> {:cont, %{state | clipboard: text, message: "copied"}}
    end
  end

  defp command(state, :cut, burst) do
    {:cont, copied} = command(state, :copy, burst)
    {:cont, change(copied, &Buffer.delete_selection/1) |> follow()}
  end

  defp command(%{clipboard: nil} = state, :paste, _burst), do: {:cont, %{state | message: "nothing to paste"}}

  defp command(state, :paste, _burst),
    do: {:cont, change(state, &Buffer.insert(&1, state.clipboard)) |> follow()}

  defp command(state, :select_all, _burst), do: {:cont, buffer(state, &Buffer.select_all/1)}

  defp command(state, :previous, _burst),
    do: {:cont, %{state | at: rem(state.at - 1 + length(state.docs), length(state.docs))}}

  defp command(state, :next, _burst), do: {:cont, %{state | at: rem(state.at + 1, length(state.docs))}}

  # A dialog the editor opened has been answered.
  defp answered(state, {:guard, command}, true), do: command(%{state | deaf_until: nil}, command, false)
  defp answered(state, {:guard, _command}, _no), do: {:cont, state}
  defp answered(state, :overwrite, true), do: save(state, :any)
  # Closed only once saved: a save that failed, or asks first, leaves the file open.
  defp answered(state, :unsaved, true) do
    {:cont, saved} = save(state, current(state).digest)
    if Buffer.modified?(current(saved).buffer), do: {:cont, saved}, else: closed(saved)
  end

  defp answered(state, :unsaved, false), do: closed(state)
  defp answered(state, :open, path) when is_binary(path) and path != "", do: open_another(state, path)

  defp answered(state, :find, text) when is_binary(text) and text != "",
    do: with_pattern(state, text, &{:cont, find(&1) |> follow()})

  defp answered(state, :replace, text) when is_binary(text) and text != "" do
    with_pattern(state, text, fn state ->
      {:cont, ask(state, Dialogs.prompt(:replacement, "Replace", "With:", state.replacement || ""))}
    end)
  end

  defp answered(state, :replacement, text) when is_binary(text) do
    {buffer, lines} = Buffer.replace_all(current(state).buffer, state.pattern, text)
    state = put_doc(%{state | replacement: text}, %{current(state) | buffer: buffer}) |> unmarked()
    {:cont, %{state | message: "replaced on #{lines} line(s)"} |> follow()}
  end

  defp answered(state, :go_to, text) when is_binary(text) do
    case Integer.parse(String.trim(text)) do
      {n, ""} when n > 0 -> {:cont, buffer(state, &Buffer.move(&1, {:to, n - 1, 0})) |> follow()}
      _ -> {:cont, %{state | message: "not a line number"}}
    end
  end

  defp answered(state, _id, _value), do: {:cont, state}

  defp ask(state, dialog), do: %{state | dialogs: Dialogs.push(state.dialogs, dialog)}
  defp deafen(state), do: %{state | deaf_until: System.monotonic_time(:millisecond) + @deaf_ms}

  # /like this/ is a Regex; anything else is the text itself.
  defp with_pattern(state, text, then) do
    case Regex.run(~r{^/(.+)/$}s, text) do
      [_, source] ->
        case Regex.compile(source, "u") do
          {:ok, re} -> then.(%{state | pattern: re})
          {:error, _} -> {:cont, %{state | message: "not a regular expression: " <> source}}
        end

      nil ->
        then.(%{state | pattern: text})
    end
  end

  defp source(nil), do: ""
  defp source(%Regex{source: source}), do: "/" <> source <> "/"
  defp source(text), do: text

  defp find(state) do
    case Buffer.find(current(state).buffer, state.pattern) do
      {:ok, buffer} -> put_doc(state, %{current(state) | buffer: buffer})
      :none -> %{state | message: "not found"}
    end
  end

  defp save(state, expected) do
    doc = current(state)

    cond do
      doc.readonly ->
        {:cont, %{state | message: read_only(doc)}}

      true ->
        case Files.save(doc.path, Buffer.text(doc.buffer), expected) do
          {:ok, digest} ->
            doc = %{doc | buffer: Buffer.mark_saved(doc.buffer), digest: digest}
            {:cont, %{put_doc(state, doc) | message: "saved " <> Path.basename(doc.path)}}

          {:error, :changed} ->
            text = "#{Path.basename(doc.path)} changed on disk since it was read. Overwrite it?"
            {:cont, ask(state, Dialogs.confirm(:overwrite, "Save", text))}

          {:error, reason} ->
            {:cont, ask(state, Dialogs.message(:failed, "Save", "Not saved: #{format(reason)}"))}
        end
    end
  end

  defp close(state) do
    doc = current(state)

    if Buffer.modified?(doc.buffer) and doc.readonly == false do
      text = "Save the changes to #{Path.basename(doc.path)}?"
      {:cont, ask(state, Dialogs.confirm(:unsaved, "Close", text))}
    else
      closed(state)
    end
  end

  # The current file goes; the editor ends with the last.
  defp closed(%{docs: [_one]}), do: {:halt, :ok}

  defp closed(state) do
    docs = List.delete_at(state.docs, state.at)
    {:cont, %{state | docs: docs, at: min(state.at, length(docs) - 1)}}
  end

  defp open_another(state, path) do
    case open(path) do
      {:ok, doc} ->
        {:cont, %{state | docs: state.docs ++ [doc], at: length(state.docs)}}

      {:error, reason} ->
        {:cont, ask(state, Dialogs.message(:failed, "Open", "Not opened: #{format(reason)}"))}
    end
  end

  defp format({:too_large, size, max}), do: "#{size} bytes, more than the #{max} the editor holds"
  defp format({:not_a_file, type}), do: "not a file (#{type})"
  defp format(reason), do: inspect(reason)

  # ---- the current file ----

  defp current(state), do: Enum.at(state.docs, state.at)
  defp put_doc(state, doc), do: %{state | docs: List.replace_at(state.docs, state.at, doc)}

  defp buffer(state, fun), do: put_doc(state, %{current(state) | buffer: fun.(current(state).buffer)})

  # An edit, unless the file is read only.
  defp change(state, fun) do
    if current(state).readonly, do: %{state | message: read_only(current(state))}, else: buffer(state, fun)
  end

  defp read_only(%{readonly: :not_utf8}), do: "read only: not UTF-8"
  defp read_only(%{readonly: :viewing}), do: "read only: viewing"

  # The view scrolled so the cursor shows.
  defp follow(state) do
    doc = current(state)
    {cols, rows} = state.size
    {height, width} = {max(rows - 2, 1), max(cols, 1)}
    {row, col} = Buffer.cursor(doc.buffer)
    at = View.column(Buffer.line(doc.buffer), col)
    top = doc.top |> min(row) |> max(row - height + 1)
    left = doc.left |> min(at) |> max(at - width + 1)
    put_doc(state, %{doc | top: top, left: left})
  end

  # ---- highlighting ----

  # The current file's start states, kept down to the window's top. An edit changes lines from
  # the cursor's, before it or after, whichever is higher up, so the states below that go. A
  # replace through the whole file, an undo and a redo (whose cursor is where it was, not where
  # the step changed the text) drop them all, with `unmarked/1`.
  defp highlighted(state) do
    doc = current(state)
    if doc.lang == nil, do: state, else: put_doc(state, marked(doc))
  end

  defp marked(doc) do
    {row, _col} = Buffer.cursor(doc.buffer)
    {version, seen} = doc.seen
    stale = fn {k, _start} -> k * @every > min(row, seen) end
    marks = if doc.buffer.version == version, do: doc.marks, else: Map.reject(doc.marks, stale)
    last = marks |> Map.keys() |> Enum.max()
    want = div(doc.top, @every)

    marks =
      if want > last do
        doc.buffer
        |> Buffer.slice(last * @every, (want - last) * @every)
        |> Enum.chunk_every(@every)
        |> Enum.with_index(last + 1)
        |> Enum.reduce(marks, fn {lines, k}, marks ->
          Map.put(marks, k, Syntax.after_lines(doc.lang, lines, marks[k - 1]))
        end)
      else
        marks
      end

    %{doc | marks: marks, seen: {doc.buffer.version, row}}
  end

  defp unmarked(state), do: put_doc(state, %{current(state) | marks: %{0 => Syntax.start()}})

  # Each of the window's lines cut into its parts, from the start state at or above its top.
  defp pieces(%{lang: nil}, lines), do: Enum.map(lines, fn _ -> nil end)

  defp pieces(doc, lines) do
    k = min(div(doc.top, @every), doc.marks |> Map.keys() |> Enum.max())
    above = Buffer.slice(doc.buffer, k * @every, doc.top - k * @every)
    start = Syntax.after_lines(doc.lang, above, doc.marks[k])
    {pieces, _state} = Enum.map_reduce(lines, start, &Syntax.line(doc.lang, &1, &2))
    pieces
  end

  # ---- drawing ----

  @impl Redoubt.Screen
  def view(state, buffer, {cols, rows}) do
    [_bar, body, status] = Layout.split({0, 0, cols, rows}, :rows, [{:fixed, 1}, :rest, {:fixed, 1}])
    doc = current(state)
    {_x, y, w, h} = body

    lines = Buffer.slice(doc.buffer, doc.top, h)

    lines
    |> Enum.zip(pieces(doc, lines))
    |> Enum.with_index(doc.top)
    |> Enum.each(fn {{line, pieces}, row} ->
      draw_line(buffer, state, doc, {line, pieces}, row, y + row - doc.top, w)
    end)

    Widgets.status(buffer, status, status_text(state, doc), Theme.style(state.theme, :status))
    MenuBar.draw(state.bar, buffer, {0, 0, cols, rows - 1}, state.theme)
    Dialogs.draw(state.dialogs, buffer, {0, 0, cols, rows}, state.theme)
  end

  defp draw_line(buffer, state, doc, {line, pieces}, row, y, w) do
    {crow, ccol} = Buffer.cursor(doc.buffer)
    cursor = if row == crow, do: ccol
    {selected, to_end} = selected_on(Buffer.selection(doc.buffer), row, String.length(line))

    for {x, text, role} <- View.runs(line, doc.left, w, selected, cursor, to_end, pieces) do
      Cells.put(buffer, x, y, text, Theme.style(state.theme, role))
    end
  end

  # The graphemes of `row` the selection covers, and whether it runs on past the row's end.
  defp selected_on(nil, _row, _length), do: {nil, false}
  defp selected_on({{r1, _}, {r2, _}}, row, _length) when row < r1 or row > r2, do: {nil, false}

  defp selected_on({{r1, c1}, {r2, c2}}, row, length) do
    from = if row == r1, do: c1, else: 0
    to = if row == r2, do: c2, else: length
    range = if to > from, do: from..(to - 1)//1, else: nil
    {range, row < r2}
  end

  defp status_text(state, doc) do
    {row, col} = Buffer.cursor(doc.buffer)
    name = Text.visible(Path.basename(doc.path))

    flags = [
      if(Buffer.modified?(doc.buffer), do: " [modified]", else: ""),
      if(doc.readonly, do: " [read only]", else: "")
    ]

    files = if length(state.docs) > 1, do: " (#{state.at + 1} of #{length(state.docs)})", else: ""
    where = " #{row + 1}:#{col + 1}"
    message = if state.message, do: "  " <> state.message, else: "  " <> @hint
    IO.iodata_to_binary([name, flags, files, where, message])
  end
end
