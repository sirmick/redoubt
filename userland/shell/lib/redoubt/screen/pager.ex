defmodule Redoubt.Screen.Pager do
  @moduledoc """
  The pager (docs/userland/shell.md, "Session commands and the pager"): a long `%Lines{}`, the
  value of a line at the prompt, shown a screen at a time on a screen of its own.

  `show/2` decides. Lines that fit the screen, or any lines where there is no terminal of known
  size to page on (a console that does not say its size, a captured output, a VM without the
  screen buffer), are printed as they are. Otherwise the pager takes the screen, with what was read so far and the rest still
  unread: it asks for more only as far as it shows or searches, so `cat("big.log")` reads what
  is looked at. The lines stay with the process that started reading them, the line's
  evaluator, which hands them over a batch at a time (`Redoubt.Screen.serve/5`), since a file
  that `cat` opened is that process's to read; when the pager ends, it stops the reading, and the
  file is closed. What the pager has read it keeps, to page back, under the line's own heap
  limit.

  Keys: Space, `f` or Page Down a page on, `b` or Page Up a page back; Down, `j` or Enter a row
  on, Up or `k` a row back; `g` or Home the top, `G` or End the end; `/`, a text and Enter
  searches forward, `n` and `N` find the next and the one before; `q` or Esc leaves, with `nil`.
  """

  @behaviour Redoubt.Screen

  alias Redoubt.Screen
  alias Redoubt.Screen.Pager.Doc
  alias Redoubt.Screen.Widgets
  alias Redoubt.Term.{Buffer, Text, Width}
  alias Redoubt.Util.Lines

  # Lines handed over at once.
  @batch 256

  @doc """
  Shows `lines`: in the pager when they are longer than the terminal's screen, else through
  `print`, a function writing a list of lines. Returns `:ok`.
  """
  @spec show(Lines.t(), ([String.t()] -> any())) :: :ok
  def show(%Lines{} = lines, print) do
    with true <- Buffer.available?(),
         {:ok, {cols, rows}} <- Screen.terminal_size(),
         {:more, read, source} <- first_screen({:start, lines}, cols, rows - 1, [], 0) do
      {_value, source} = Screen.serve(__MODULE__, {read, lines.style}, &hand_over/2, source)
      halt(source)
    else
      {:fits, read} -> print.(read)
      _no_terminal -> lines |> Stream.chunk_every(@batch) |> Enum.each(print)
    end

    :ok
  end

  # What a screen of `room` rows holds of the lines: all of them, or the first and the rest.
  defp first_screen(source, cols, room, read, used) do
    case pull(source) do
      :done ->
        {:fits, Enum.reverse(read)}

      {line, source} ->
        used = used + length(Doc.wrap(Text.visible(line), cols, 0))

        if used > room,
          do: {:more, Enum.reverse([line | read]), source},
          else: first_screen(source, cols, room, [line | read], used)
    end
  end

  # The evaluator's side: the next batch, and whether it is the last.
  defp hand_over({:more, n}, source) do
    {batch, source} = pull_many(source, n, [])
    {{batch, source == :done}, source}
  end

  # ---- reading, a line at a time: a suspended enumeration ----

  defp pull(:done), do: :done

  defp pull({:start, enum}),
    do: step(Enumerable.reduce(enum, {:cont, nil}, fn line, _ -> {:suspend, line} end))

  defp pull({:cont, next}), do: step(next.({:cont, nil}))

  defp step({:suspended, line, next}), do: {line, {:cont, next}}
  defp step({done, _acc}) when done in [:done, :halted], do: :done

  defp pull_many(source, 0, acc), do: {Enum.reverse(acc), source}

  defp pull_many(source, n, acc) do
    case pull(source) do
      :done -> {Enum.reverse(acc), :done}
      {line, source} -> pull_many(source, n - 1, [line | acc])
    end
  end

  defp halt({:cont, next}), do: next.({:halt, nil})
  defp halt(_done), do: :ok

  # ---- the screen ----

  @impl Screen
  def init({read, style}) do
    %{doc: Doc.push(Doc.new(style), read), top: {0, 0}, size: {80, 24}, search: nil, query: nil, note: nil}
  end

  @impl Screen
  def update({:resize, cols, rows}, state) do
    state = %{state | size: {cols, rows}}
    {:cont, state |> fill(elem(state.top, 0) + rows) |> clamp()}
  end

  def update({:key, key, mods}, %{search: typed} = state) when typed != nil, do: searching(state, key, mods)
  def update({:key, key, _mods}, state), do: key(state, key)
  def update(_message, state), do: {:cont, state}

  defp key(state, key) when key in [" ", "f", :page_down], do: {:cont, down(state, page(state))}
  defp key(state, key) when key in ["b", :page_up], do: {:cont, up(state, page(state))}
  defp key(state, key) when key in [:down, "j", :enter], do: {:cont, down(state, 1)}
  defp key(state, key) when key in [:up, "k"], do: {:cont, up(state, 1)}
  defp key(state, key) when key in ["g", :home], do: {:cont, %{state | top: {0, 0}, note: nil}}
  defp key(state, key) when key in ["G", :end], do: {:cont, state |> fill(:all) |> to_end()}
  defp key(state, "/"), do: {:cont, %{state | search: "", note: nil}}
  defp key(%{query: q} = state, "n") when q != nil, do: {:cont, find(state, :next)}
  defp key(%{query: q} = state, "N") when q != nil, do: {:cont, find(state, :prev)}
  defp key(_state, key) when key in ["q", :esc], do: {:halt, nil}
  defp key(state, _other), do: {:cont, state}

  # The search being typed, on the status line.
  defp searching(state, :enter, _mods) when state.search != "",
    do: {:cont, find(%{state | query: state.search, search: nil}, :next)}

  defp searching(state, :enter, _mods), do: {:cont, %{state | search: nil}}
  defp searching(state, :esc, _mods), do: {:cont, %{state | search: nil}}

  defp searching(state, :backspace, _mods),
    do: {:cont, %{state | search: String.slice(state.search, 0..-2//1)}}

  defp searching(state, key, []) when is_binary(key), do: {:cont, %{state | search: state.search <> key}}
  defp searching(state, _key, _mods), do: {:cont, state}

  defp page(%{size: {_cols, rows}}), do: max(rows - 2, 1)
  defp body(%{size: {_cols, rows}}), do: max(rows - 1, 1)
  defp cols(%{size: {cols, _rows}}), do: cols

  defp down(state, n) do
    {line, _row} = state.top
    state = fill(state, line + n + body(state))
    clamp(%{state | top: Doc.down(state.doc, state.top, n, cols(state)), note: nil})
  end

  defp up(state, n), do: %{state | top: Doc.up(state.doc, state.top, n, cols(state)), note: nil}

  # Past the end there is nothing to show: once the end is read, the last row stays at the bottom.
  defp clamp(%{doc: %{ended: true}} = state),
    do: %{state | top: min(state.top, Doc.last(state.doc, cols(state), body(state)))}

  defp clamp(state), do: state

  defp to_end(state), do: %{state | top: Doc.last(state.doc, cols(state), body(state))}

  # Lines read until line `upto` is, or all of them; the evaluator hands them over.
  defp fill(%{doc: %{ended: true}} = state, _upto), do: state

  defp fill(state, upto) do
    if upto != :all and Doc.count(state.doc) > upto do
      state
    else
      {batch, ended} = Screen.call({:more, @batch})
      fill(%{state | doc: Doc.push(state.doc, batch, ended)}, upto)
    end
  end

  # The next line holding the query, after the top line or before it. Forward, it reads on a
  # batch at a time until it is found or the lines end, looking only at what each batch added.
  defp find(state, direction), do: find(state, elem(state.top, 0), direction)

  defp find(state, from, direction) do
    case {Doc.find(state.doc, state.query, from, direction), direction, state.doc.ended} do
      {nil, :next, false} ->
        read = Doc.count(state.doc)
        state |> fill(read + @batch - 1) |> find(read - 1, :next)

      {nil, _direction, _ended} ->
        %{state | note: "not found: " <> state.query}

      {found, _direction, _ended} ->
        clamp(%{state | top: {found, 0}, note: nil})
    end
  end

  @impl Screen
  def view(state, buffer, {cols, rows}) do
    h = body(state)
    shown = Doc.view(state.doc, state.top, cols, h)

    shown
    |> Enum.with_index()
    |> Enum.each(fn {{_line, text, bold}, y} -> row(buffer, y, text, bold, state.query) end)

    Widgets.status(buffer, {0, rows - 1, cols, 1}, status(state, shown))
  end

  # A row, and each match of the query in it reversed. The text is already visible.
  defp row(buffer, y, text, bold, query) do
    if text != "", do: Buffer.put(buffer, 0, y, text, Buffer.style(bold: bold))

    if query not in [nil, ""] do
      for {at, len} <- :binary.matches(text, query) do
        x = Width.columns(binary_part(text, 0, at))
        Buffer.put(buffer, x, y, binary_part(text, at, len), Buffer.style(bold: bold, reversed: true))
      end
    end
  end

  defp status(%{search: typed}, _shown) when typed != nil, do: "/" <> typed
  defp status(%{note: note}, _shown) when note != nil, do: note

  defp status(state, shown) do
    where =
      case shown do
        [] -> "no lines"
        [{first, _, _} | _] -> "lines #{first + 1}-#{elem(List.last(shown), 0) + 1}" <> total(state.doc)
      end

    at_end = state.doc.ended and state.top >= Doc.last(state.doc, cols(state), body(state))
    where <> if(at_end, do: " (END)", else: "") <> " · Space next · b back · / search · q quit"
  end

  defp total(%{ended: true} = doc), do: " of #{Doc.count(doc)}"
  defp total(_doc), do: ""
end
