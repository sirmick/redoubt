defmodule Redoubt.Editor.Manager do
  @moduledoc """
  The file manager (docs/userland/shell.md, "The editor"): `fm(dir)`, two panes of directories
  in the manner of Midnight Commander, and the editor in front of them for a file.

  Each pane lists one directory through `Redoubt.Editor.Files`: Tab moves between them, Enter
  goes into a directory (or up, on `..`, through the pane's own path, never a listed name), and
  on a file edits it. F3 views a file and F4 edits it, in `Redoubt.Editor`; closing the editor
  comes back to the panes. F5 copies and F6 moves the selected name to the other pane's
  directory, F7 makes a directory and F8 removes; F10 or Ctrl+Q leaves.

  The panes act only on what they list, and only as `Redoubt.Editor.Files` allows: a name it
  refuses is shown and acted on by nothing. Copy, move and remove ask first, and a question is
  deaf until 300 ms after the last of the keys that arrive with it, so a paste cannot answer it.
  """

  use Redoubt.Commandlet, area: "Screens"

  @behaviour Redoubt.Screen

  alias Redoubt.Editor
  alias Redoubt.Editor.Files
  alias Redoubt.Screen.{Layout, Widgets}
  alias Redoubt.Screen.Widget.{Dialogs, List, Theme}

  # Keys arriving this long after a question that acts on files opened are dropped.
  @deaf_ms 300

  @hint "Tab pane · Enter open · F3 view · F4 edit · F5 copy · F6 move · F7 mkdir · F8 remove · F10 quit"

  @summary "Manage files in two panes, on a screen"
  @help """
  Opens two panes on dir, in the manner of Midnight Commander. Tab moves between the panes, Enter
  goes into a directory (or up, on ..) and edits a file. F3 views a file and F4 edits it, in the
  editor (ed); closing the editor comes back to the panes. F5 copies and F6 moves the selected
  name into the other pane's directory, F7 makes a directory, F8 removes a file or a directory and
  all it holds; each of these asks first. F10 or Ctrl+Q leaves, and so does Ctrl+\\.

  A name that holds / or NUL, or is . or .., is listed with a ! and nothing acts on it. Nothing is
  overwritten, and no directory is copied or moved into itself.
  """
  @args dir: "the directory both panes start in"
  @examples [{~S'fm()', "manage the files here"}, {~S'fm("/data")', "manage the files in /data"}]
  defcommand fm(dir :: path \\ ".") do
    dir = Path.expand(dir)

    if File.dir?(dir),
      do: Redoubt.Screen.run(__MODULE__, dir, ctrl_c: :key),
      else: {:error, {:not_a_directory, dir}}
  end

  # ---- the screen ----

  @impl Redoubt.Screen
  def init(dir) do
    %{
      panes: [pane(dir), pane(dir)],
      at: 0,
      dialogs: Dialogs.new(),
      theme: Theme.get(:plain),
      size: {80, 24},
      message: nil,
      editor: nil,
      deaf_until: nil
    }
  end

  @impl Redoubt.Screen
  def update({:resize, cols, rows} = event, state) do
    editor = if state.editor, do: elem(Editor.update(event, state.editor), 1)
    {:cont, %{state | size: {cols, rows}, editor: editor}}
  end

  def update(event, %{editor: editor} = state) when editor != nil do
    case Editor.update(event, editor) do
      {:cont, editor} -> {:cont, %{state | editor: editor}}
      {:halt, _value} -> {:cont, refresh(%{state | editor: nil})}
    end
  end

  def update({:key, _, _} = key, state) do
    now = System.monotonic_time(:millisecond)

    cond do
      deaf?(state, now) -> {:cont, deafened(state, now)}
      Dialogs.open?(state.dialogs) -> dialog_key(state, key)
      true -> pane_key(%{state | message: nil}, key)
    end
  end

  def update(_message, state), do: {:cont, state}

  defp deaf?(%{deaf_until: nil}, _now), do: false
  defp deaf?(state, now), do: now < state.deaf_until

  # A key dropped with more queued behind it is a burst still arriving: the question stays deaf
  # until 300 ms after its last key, however long the burst.
  defp deafened(state, now) do
    {:message_queue_len, queued} = Process.info(self(), :message_queue_len)
    if queued > 0, do: %{state | deaf_until: now + @deaf_ms}, else: state
  end

  defp dialog_key(state, key) do
    case Dialogs.key(state.dialogs, key) do
      {:cont, dialogs} -> {:cont, %{state | dialogs: dialogs}}
      {:closed, id, value, dialogs} -> answered(%{state | dialogs: dialogs, deaf_until: nil}, id, value)
      :pass -> {:cont, state}
    end
  end

  # ---- the panes ----

  # A pane: its directory, what it lists, and the list drawn. `..` comes first, but at `/`.
  defp pane(dir, selected_name \\ nil) do
    {entries, message} =
      case Files.list(dir) do
        {:ok, entries} -> {entries, nil}
        {:error, reason} -> {[], "cannot list #{Path.basename(dir)}: #{inspect(reason)}"}
      end

    entries = if dir == "/", do: entries, else: [:up | entries]
    selected = Enum.find_index(entries, &(is_map(&1) and &1.name == selected_name)) || 0

    %{
      dir: dir,
      entries: entries,
      list: List.new(Enum.map(entries, &label/1), selected: selected),
      error: message
    }
  end

  defp label(:up), do: "/.."
  defp label(%{ok: false, shown: shown}), do: "!" <> shown
  defp label(%{type: :directory, shown: shown}), do: "/" <> shown
  defp label(%{type: :regular, shown: shown}), do: " " <> shown
  defp label(%{shown: shown}), do: "?" <> shown

  # Both panes listed again, each keeping its selection where its name still is.
  defp refresh(state) do
    panes = Enum.map(state.panes, fn p -> pane(p.dir, name(selected(p))) end)
    %{state | panes: panes}
  end

  defp name(%{name: name}), do: name
  defp name(_up), do: nil

  defp current(state), do: Enum.at(state.panes, state.at)
  defp other(state), do: Enum.at(state.panes, 1 - state.at)
  defp put_pane(state, pane), do: %{state | panes: Elixir.List.replace_at(state.panes, state.at, pane)}
  defp selected(pane), do: Enum.at(pane.entries, pane.list.selected)

  defp pane_key(state, {:key, key, mods} = k) do
    entry = selected(current(state))

    case {key, mods} do
      {:tab, _} ->
        {:cont, %{state | at: 1 - state.at}}

      {{:f, 10}, []} ->
        {:halt, :ok}

      {"q", [:ctrl]} ->
        {:halt, :ok}

      {:enter, []} ->
        open(state, entry)

      {{:f, 3}, []} ->
        edit(state, entry, view: true)

      {{:f, 4}, []} ->
        edit(state, entry, [])

      {{:f, 5}, []} ->
        ask_on(state, entry, :copy)

      {{:f, 6}, []} ->
        ask_on(state, entry, :move)

      {{:f, 7}, []} ->
        {:cont, ask(state, Dialogs.prompt({:mkdir, current(state).dir}, "Make a directory", "Its name:"))}

      {{:f, 8}, []} ->
        ask_on(state, entry, :remove)

      _ ->
        list_key(state, k)
    end
  end

  defp list_key(state, key) do
    pane = current(state)

    case List.key(pane.list, key) do
      {:cont, list} -> {:cont, put_pane(state, %{pane | list: list})}
      _done_or_pass -> {:cont, state}
    end
  end

  # Enter: up through the pane's own path, into a listed directory, or into the editor.
  defp open(state, :up),
    do: {:cont, put_pane(state, pane(Path.dirname(current(state).dir), Path.basename(current(state).dir)))}

  defp open(state, %{ok: true, type: :directory, name: name}),
    do: {:cont, put_pane(state, pane(Path.join(current(state).dir, name)))}

  defp open(state, %{type: :regular} = entry), do: edit(state, entry, [])
  defp open(state, entry), do: {:cont, refused(state, entry)}

  defp edit(state, %{ok: true, type: :regular, name: name}, opts) do
    case Editor.open(Path.join(current(state).dir, name), opts) do
      {:ok, doc} ->
        {cols, rows} = state.size
        {:cont, editor} = Editor.update({:resize, cols, rows}, Editor.init(doc))
        {:cont, %{state | editor: editor}}

      {:error, reason} ->
        {:cont, %{state | message: "not opened: #{inspect(reason)}"}}
    end
  end

  defp edit(state, entry, _opts), do: {:cont, refused(state, entry)}

  defp refused(state, %{ok: false, shown: shown}),
    do: %{state | message: "#{shown}: a name with / or NUL, or . or .., is acted on by nothing"}

  defp refused(state, _entry), do: %{state | message: "not a file"}

  # Copy, move and remove ask first, naming what is listed and where it goes; the question is
  # deaf for a moment.
  defp ask_on(state, %{ok: true, name: name, shown: shown}, action) do
    from = current(state).dir
    to = other(state).dir

    text =
      case action do
        :copy -> "Copy #{shown} into #{visible(to)}?"
        :move -> "Move #{shown} into #{visible(to)}?"
        :remove -> "Remove #{shown}, and all it holds?"
      end

    dialog = Dialogs.confirm({action, from, name, to}, String.capitalize(to_string(action)), text)
    {:cont, %{ask(state, dialog) | deaf_until: System.monotonic_time(:millisecond) + @deaf_ms}}
  end

  defp ask_on(state, entry, _action), do: {:cont, refused(state, entry)}

  defp visible(path), do: Redoubt.Term.Text.visible(path)
  defp ask(state, dialog), do: %{state | dialogs: Dialogs.push(state.dialogs, dialog)}

  defp answered(state, {:copy, from, name, to}, true), do: done(state, Files.copy(from, name, to))
  defp answered(state, {:move, from, name, to}, true), do: done(state, Files.move(from, name, to))
  defp answered(state, {:remove, from, name, _to}, true), do: done(state, Files.remove(from, name))

  defp answered(state, {:mkdir, dir}, name) when is_binary(name) and name != "",
    do: done(state, Files.mkdir(dir, name))

  defp answered(state, _id, _value), do: {:cont, state}

  defp done(state, :ok), do: {:cont, refresh(state)}

  defp done(state, {:error, reason}),
    do: {:cont, refresh(ask(state, Dialogs.message(:failed, "Not done", format(reason))))}

  defp format(:eexist), do: "something of that name is there already"
  defp format(:einval), do: "a directory cannot go into itself"
  defp format(:bad_name), do: "a name with / or NUL, or . or .., is acted on by nothing"
  defp format(reason), do: inspect(reason)

  # ---- drawing ----

  @impl Redoubt.Screen
  def view(%{editor: editor}, buffer, size) when editor != nil, do: Editor.view(editor, buffer, size)

  def view(state, buffer, {cols, rows}) do
    [body, status] = Layout.split({0, 0, cols, rows}, :rows, [:rest, {:fixed, 1}])
    rects = Layout.split(body, :cols, [:rest, :rest])

    for {{pane, rect}, i} <- state.panes |> Enum.zip(rects) |> Enum.with_index() do
      draw_pane(pane, rect, buffer, state.theme, i == state.at)
    end

    Widgets.status(buffer, status, status_text(state), Theme.style(state.theme, :status))
    Dialogs.draw(state.dialogs, buffer, {0, 0, cols, rows}, state.theme)
  end

  defp draw_pane(pane, {x, y, w, h} = rect, buffer, theme, focused) do
    Widgets.box(buffer, rect, title: pane.dir, style: Theme.style(theme, :border), shadow: false)
    inner = {x + 1, y + 1, max(w - 2, 0), max(h - 2, 0)}
    list = List.page(pane.list, max(h - 2, 1))
    List.draw(list, buffer, inner, theme, focused)
  end

  defp status_text(state) do
    pane = current(state)
    state.message || pane.error || @hint
  end
end
