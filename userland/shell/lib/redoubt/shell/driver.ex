defmodule Redoubt.Shell.Driver do
  @moduledoc """
  The shell's terminal driver: the one process holding the session's console, with OTP's
  `group` and `edlin` above it, unchanged, and `Redoubt.Term` drawing for it
  (docs/userland/shell.md, "Line editing and history"). It replaces `user_drv` and `prim_tty`.

  Bytes typed go to `group` as they are, for `edlin` to edit with, but for the two keys the
  session keeps: Ctrl+C ends the line being edited, not the session, and Ctrl+D on an empty
  line ends the input, and with it the shell. Whatever `group` asks to draw is drawn by
  `Redoubt.Term`, and so passes its guard; what it asks about the terminal (its size, its
  encoding) is answered here. The driver ends when `group` does, which is when the shell has.

  History is `group`'s own, in the session's memory only. `group` keeps every line, so the
  driver cuts its list to the newest lines at each prompt.

  A screen (`Redoubt.Screen`) takes the terminal while it is in front: the driver shows the
  alternate screen, decodes the bytes typed into keys (`Redoubt.Term.Keys`) and sends them to the
  screen's process, draws each frame it sends through the one decoder (`Redoubt.Term.Cells`) and
  `Redoubt.Term.Frame`, and holds what `group` asks to draw, answering it at once so a writer
  never waits on the screen. Ctrl+C ends the screen. When the screen ends, the main screen is
  shown again, as it was, and what was held is drawn.
  """

  alias Redoubt.Term
  alias Redoubt.Term.{Cells, Frame, Keys}

  require Record

  # group's state, for its history alone: the field is named, so an OTP whose group keeps its
  # history elsewhere fails this build instead of having the wrong field cut.
  Record.defrecordp(:group_state, :state, Record.extract(:state, from_lib: "kernel/src/group.erl"))

  @history_lines 1000
  @esc_timeout 50

  @doc """
  Runs the driver in the calling process until the shell ends, and returns `:ok`.

  Options:
  - `:shell` (`{Redoubt.Shell, :start_link, [[]]}`): the shell `group` starts, as a call
    returning its pid.
  - `:input` (`:console`): where keys come from. `:console` takes the console with
    `:beamlet.console_subscribe/0`; with `:messages`, whoever runs the driver sends it
    `{:beamlet_console, bytes}` and `{:beamlet_console, :eof}` itself, as the tests do.
  - `:output`: a function writing bytes to the terminal; the console by default.
  - `:size`: a function giving the terminal's size as `{cols, rows}`; the console's by default,
    80 by 24 when it is unknown. It is asked at each prompt.
  - `:history_lines` (#{@history_lines}): how many of the newest lines history keeps.
  - `:esc_timeout` (#{@esc_timeout}): with a screen in front, how many milliseconds a lone ESC
    waits for more before it is the Esc key.
  """
  @spec run(keyword()) :: :ok
  def run(opts \\ []) do
    Process.flag(:trap_exit, true)
    shell = Keyword.get(opts, :shell, {Redoubt.Shell, :start_link, [[]]})
    output = Keyword.get_lazy(opts, :output, fn -> console_writer() end)
    size = Keyword.get(opts, :size, &console_size/0)

    # `beamlet` is the VM's own module, there on beamlet alone: called by name, so the BEAM,
    # where the tests also run, compiles this without a word.
    if Keyword.get(opts, :input, :console) == :console do
      :ok = apply(:beamlet, :console_subscribe, [])
    end

    {cols, rows} = size.()
    group = :group.start(self(), shell, echo: true, expand_below: true, expand_fun: &expand/1)

    loop(%{
      group: group,
      term: Term.new(cols, rows),
      output: output,
      size: size,
      history: Keyword.get(opts, :history_lines, @history_lines),
      # Bytes of a UTF-8 sequence the read cut, waiting for the rest.
      held: <<>>,
      # The last byte sent to group.
      last: nil,
      # Whether group's drawing is the dropped line's (after an interrupt).
      dropping: false,
      # A Ctrl+D waiting for group to catch up: its reference, the rest of its read, and the
      # input that came after it, newest first.
      eof_check: nil,
      # The screen in front, if any: its process, monitor, what group asked to draw meanwhile
      # (newest first), bytes that may begin a key, and the timer that ends their wait.
      screen: nil,
      esc_timeout: Keyword.get(opts, :esc_timeout, @esc_timeout)
    })
  end

  defp loop(%{group: group} = state) do
    receive do
      {:beamlet_console, input} when state.screen != nil ->
        loop(screen_input(state, input))

      {:beamlet_console, input} ->
        loop(input(state, input))

      {:redoubt_screen, :open, pid} ->
        loop(open_screen(state, pid))

      {:redoubt_screen, :frame, pid, bytes} when state.screen != nil and state.screen.pid == pid ->
        loop(frame(state, bytes))

      {:redoubt_screen, :close, pid} when state.screen != nil and state.screen.pid == pid ->
        loop(close_screen(state))

      {:DOWN, ref, :process, _pid, _reason} when state.screen != nil and state.screen.ref == ref ->
        loop(close_screen(state))

      {:timeout, timer, :keys_flush} when state.screen != nil and state.screen.timer == timer ->
        loop(flush_keys(state))

      # What a screen that has ended still sent, and a timer that was overtaken: nothing now.
      {:timeout, _stale, :keys_flush} ->
        loop(state)

      {:redoubt_screen, _what, _pid} ->
        loop(state)

      {:redoubt_screen, _what, _pid, _detail} ->
        loop(state)

      {:io_reply, ref, _reply} when elem(state.eof_check, 0) == ref ->
        loop(eof_checked(state))

      {:io_reply, _ref, _reply} ->
        loop(state)

      {^group, {:put_chars_sync, _encoding, _chars, reply} = request} ->
        state = draw(state, request)
        send(group, {:reply, reply, :ok})
        loop(state)

      {^group, :tty_geometry} ->
        {cols, rows} = state.size.()
        send(group, {self(), :tty_geometry, {cols, rows}})
        loop(%{state | term: Term.resize(state.term, cols, rows)})

      {^group, :get_unicode_state} ->
        send(group, {self(), :get_unicode_state, true})
        loop(state)

      {^group, :set_unicode_state, _bool} ->
        send(group, {self(), :set_unicode_state, true})
        loop(state)

      {^group, :get_terminal_state} ->
        send(group, {self(), :get_terminal_state, %{stdin: true, stdout: true, stderr: true}})
        loop(state)

      {^group, {:open_editor, _buffer}} ->
        send(group, {self(), :not_supported})
        loop(state)

      {^group, request} ->
        loop(draw(state, request))

      # The shell has ended, even under a screen: the main screen is shown again.
      {:EXIT, ^group, _reason} ->
        if state.screen != nil, do: write(state, Frame.leave())
        :ok

      {:EXIT, _other, _reason} ->
        loop(state)
    end
  end

  # ---- drawing ----

  # With a screen in front, what group asks to draw waits for it to end.
  defp draw(%{screen: %{} = screen} = state, request),
    do: %{state | screen: %{screen | held: [request | screen.held]}}

  # After an interrupt, what group asks to draw until the next prompt is the dropped line's:
  # the keys typed just before Ctrl+C, which group was still editing when the driver drew the
  # ^C. They are not drawn.
  defp draw(%{dropping: true} = state, request) do
    if prompting?(request), do: draw(%{state | dropping: false}, request), else: state
  end

  defp draw(state, request) do
    prompting = prompting?(request)
    if prompting, do: trim_history(state)
    term = if prompting, do: sized(state), else: state.term
    {out, term} = Term.request(term, request)
    write(state, out)
    %{state | term: term}
  end

  # A line has just ended, and group has kept it: its history is cut to the newest lines, by a
  # process of its own, since group may be waiting on the driver (for the terminal's size) when
  # the cut reaches it. The next line's read takes the history as it then is.
  defp trim_history(%{group: group, history: keep}) do
    spawn(fn ->
      :sys.replace_state(group, fn {name, data} ->
        lines = group_state(data, :line_history)
        trimmed = if is_list(lines), do: Enum.take(lines, keep), else: lines
        {name, group_state(data, line_history: trimmed)}
      end)
    end)
  end

  # A new prompt lays the line out at the terminal's size now.
  defp prompting?(:new_prompt), do: true
  defp prompting?({:requests, requests}), do: :new_prompt in requests
  defp prompting?(_request), do: false

  defp sized(state) do
    {cols, rows} = state.size.()
    Term.resize(state.term, cols, rows)
  end

  defp write(state, out) do
    if IO.iodata_length(out) > 0, do: state.output.(out)
    :ok
  end

  # ---- keys ----

  # Input waits behind a Ctrl+D group has not caught up with, in order.
  defp input(%{eof_check: {ref, rest, queued}} = state, input),
    do: %{state | eof_check: {ref, rest, [input | queued]}}

  # A last line without its newline is still a line, as it is from a pipe.
  defp input(state, :eof) do
    state = if state.last in [?\n, ?\r, nil], do: state, else: to_group(state, "\n")
    end_of_input(state)
  end

  defp input(state, bytes) when is_binary(bytes), do: typed(state, state.held <> bytes)

  # Bytes as they come from the console: a sequence cut by the read's end waits for the rest; a
  # byte that is not UTF-8 (a pasted Latin-1 character, an 8-bit Meta key) is read as the
  # Latin-1 character it is.
  defp typed(state, bytes) do
    case :unicode.characters_to_binary(bytes) do
      text when is_binary(text) ->
        keys(%{state | held: <<>>}, text)

      {:incomplete, text, rest} ->
        keys(%{state | held: rest}, text)

      {:error, text, <<byte, rest::binary>>} ->
        state = keys(%{state | held: <<>>}, text)
        typed(state, <<byte::utf8, rest::binary>>)
    end
  end

  defp keys(state, <<>>), do: state

  defp keys(state, text) do
    case :binary.match(text, [<<3>>, <<4>>]) do
      :nomatch ->
        to_group(state, text)

      {at, 1} ->
        <<ahead::binary-size(^at), key, rest::binary>> = text
        key(to_group(state, ahead), key, rest)
    end
  end

  # Ctrl+C: the line being edited is dropped and the session stays. What was typed with it in
  # the same read goes with the line, as user_drv drops it too. With no line open, a line is
  # being evaluated, and ending that is not the driver's yet (docs/userland/shell.md,
  # "Interrupting and killing jobs"): the key is dropped.
  defp key(state, 3, _rest) do
    if Term.line_open?(state.term) do
      {out, term} = Term.interrupt(state.term)
      write(state, out)
      Process.exit(state.group, :interrupt)
      %{state | term: term, dropping: true}
    else
      state
    end
  end

  # Ctrl+D: on an empty line the input ends, and the shell with it; with text on the line it
  # is edlin's forward delete. Which it is depends on the keys before it, which group may not
  # have drawn yet, so it is judged once group has: group answers a no-op io request in order,
  # after the drawing those keys asked for. Until then, the rest of the read and any input
  # after it wait.
  defp key(state, 4, rest) do
    ref = make_ref()
    send(state.group, {:io_request, self(), ref, {:setopts, []}})
    %{state | eof_check: {ref, rest, []}}
  end

  # ---- a screen in front ----

  # One screen at a time: a second is refused, and its process ends, before it has run any of
  # its module's code.
  defp open_screen(%{screen: nil} = state, pid) do
    {cols, rows} = state.size.()
    write(state, Frame.enter())
    send(pid, {:redoubt_screen, :opened, cols, rows})
    screen = %{pid: pid, ref: Process.monitor(pid), held: [], pending: <<>>, timer: nil}
    %{state | screen: screen}
  end

  defp open_screen(state, pid) do
    Process.exit(pid, :another_screen_in_front)
    state
  end

  # A frame the decoder refuses ends the screen: nothing reaches the terminal but cells.
  defp frame(state, bytes) do
    case Cells.decode(bytes) do
      {:ok, frame} ->
        write(state, Frame.draw(frame))
        state

      {:error, _why} ->
        end_screen(state, :refused_frame)
    end
  end

  # The main screen as it was, and then what group asked to draw meanwhile.
  defp close_screen(%{screen: screen} = state) do
    Process.demonitor(screen.ref, [:flush])
    cancel(screen.timer)
    write(state, Frame.leave())
    screen.held |> Enum.reverse() |> Enum.reduce(%{state | screen: nil}, &draw(&2, &1))
  end

  # Keys for the screen; Ctrl+C ends it, and the input's end ends it before ending the input.
  defp screen_input(state, :eof), do: state |> interrupt_screen() |> input(:eof)

  defp screen_input(%{screen: screen} = state, bytes) do
    cancel(screen.timer)
    {keys, pending} = Keys.decode(screen.pending <> bytes)
    state = send_keys(%{state | screen: %{screen | pending: pending, timer: nil}}, keys)

    case state.screen do
      %{pending: <<_, _::binary>>} = screen ->
        timer = :erlang.start_timer(state.esc_timeout, self(), :keys_flush)
        %{state | screen: %{screen | timer: timer}}

      _done_or_none ->
        state
    end
  end

  defp cancel(nil), do: :ok
  defp cancel(timer), do: Process.cancel_timer(timer)

  # Nothing more came after what may have begun a key: what it is, it is now.
  defp flush_keys(%{screen: screen} = state) do
    keys = Keys.flush(screen.pending)
    send_keys(%{state | screen: %{screen | pending: <<>>, timer: nil}}, keys)
  end

  defp send_keys(state, []), do: state
  defp send_keys(%{screen: nil} = state, _keys), do: state
  defp send_keys(state, [{:key, "c", [:ctrl]} | _rest]), do: interrupt_screen(state)

  defp send_keys(state, [key | rest]) do
    send(state.screen.pid, key)
    send_keys(state, rest)
  end

  # The interrupt ends the screen: its process exits with the reason `:interrupt`, which is how
  # its line's process tells the interrupt from a failure.
  defp interrupt_screen(%{screen: nil} = state), do: state
  defp interrupt_screen(state), do: end_screen(state, :interrupt)

  # The screen's process exits with `reason`; one that traps exits is killed by the second
  # signal, which arrives after the first, as signals between two processes do.
  defp end_screen(%{screen: screen} = state, reason) do
    Process.exit(screen.pid, reason)
    Process.exit(screen.pid, :kill)
    close_screen(state)
  end

  # ---- the end of the input ----

  # group has drawn every key before the Ctrl+D. On an empty line the input ends, and what
  # waited behind it goes; otherwise the Ctrl+D goes to group, and then what waited, in order.
  defp eof_checked(%{eof_check: {_ref, rest, queued}} = state) do
    state = %{state | eof_check: nil}

    if Term.line_empty?(state.term) do
      end_of_input(state)
    else
      state = state |> to_group(<<4>>) |> keys(rest)
      queued |> Enum.reverse() |> Enum.reduce(state, &input(&2, &1))
    end
  end

  # The end of the input, as group can carry it to the shell: not as `eof`, which edlin takes
  # for a line's end and loses, but as the error `eof` answering the shell's read, which group
  # holds, in order after the keys before it, until a read is pending. The shell reads it as
  # the end of its input.
  defp end_of_input(state) do
    send(state.group, {self(), {:error, :eof}})
    state
  end

  defp to_group(state, <<>>), do: state

  defp to_group(state, text) do
    send(state.group, {self(), {:data, text}})
    %{state | last: :binary.last(text)}
  end

  # ---- the console ----

  defp console_writer do
    port = :erlang.open_port({:fd, 0, 1}, [:out, :binary])
    fn bytes -> Port.command(port, bytes) end
  end

  defp console_size do
    case apply(:beamlet, :console_size, []) do
      {cols, rows} when is_integer(cols) and is_integer(rows) -> {cols, rows}
      :unknown -> {80, 24}
    end
  end

  # Completion comes with the commands' registry; until then a Tab is a beep.
  defp expand(_before), do: {:no, ~c"", []}
end
