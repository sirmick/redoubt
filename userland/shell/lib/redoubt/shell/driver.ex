defmodule Redoubt.Shell.Driver do
  @moduledoc """
  The shell's terminal driver: the one process holding the session's console, with OTP's
  `group` and `edlin` above it, unchanged, and `Redoubt.Term` drawing for it
  (docs/userland/shell.md, "Line editing and history"). It replaces `user_drv` and `prim_tty`.

  Bytes typed go to `group` as they are, for `edlin` to edit with, but for the keys the session
  keeps: the interrupt, Ctrl+C or the session's own key Ctrl+\\, ends the line being edited,
  not the session, and Ctrl+D on an empty line ends the input, and with it the shell. Ctrl+\\
  is never forwarded to anything. Whatever `group` asks to draw is drawn by `Redoubt.Term`, and
  so passes its guard; what it asks about the terminal (its size, its encoding) is answered
  here. The driver ends when `group` does, which is when the shell has.

  History is `group`'s own, in the session's memory only. `group` keeps every line, so the
  driver cuts its list to the newest lines at each prompt.

  A screen (`Redoubt.Screen`) takes the terminal while it is in front: the driver shows the
  alternate screen, decodes the bytes typed into keys (`Redoubt.Term.Keys`) and sends them to the
  screen's process, draws each frame it sends through the one decoder (`Redoubt.Term.Cells`) and
  `Redoubt.Term.Frame`, and holds what `group` asks to draw, answering it at once so a writer
  never waits on the screen. Ctrl+\\ ends the screen, and so does Ctrl+C unless the screen takes
  it as a key (`Redoubt.Screen.run/3`'s `ctrl_c: :key`). When the screen ends, the main screen is
  shown again, as it was, and what was held is drawn.

  While a line runs a native program that reads the console (docs/userland/native.md, "Standard
  input and output, and pipes"), the process feeding it holds the driver's **feed**
  (`open_feed/0`): the lines typed go to it, edited as they are typed (a backspace takes back a
  character) and echoed through `Redoubt.Term`, and Ctrl+D on an empty line is the end of its
  input, not the shell's. The interrupt keys are never fed: they stay the shell's. The feed ends
  when its process does, and what is typed after goes to `group` again.

  While a line is evaluated, the shell tells the driver (`Redoubt.Shell.Evaluator`), and the
  interrupt, with no line being edited and no screen in front, ends that line: the driver draws
  `^C` and tells the shell, which kills the line's process (docs/userland/shell.md, "Interrupting
  and killing jobs"). Under a feed the interrupt does the same, so a native program reading what
  is typed cannot keep it from the shell.

  While the driver runs, the logger writes through it too (`Redoubt.Shell.Log`): its `default`
  handler, which writes to the console past the guard, is put back when the driver ends.

  A change of the console's size arrives as `{:beamlet_console_resize, {cols, rows}}`, from the
  VM (`:beamlet` hands its platform's change to the console's reader). The size is the console's
  from then on; a screen in front is sent `{:resize, cols, rows}` and lays itself out again, and
  with none the line being edited is laid out again at the new width.
  """

  alias Redoubt.Term
  alias Redoubt.Term.{Frame, Keys}

  require Record

  # group's state, for its history alone: the field is named, so an OTP whose group keeps its
  # history elsewhere fails this build instead of having the wrong field cut.
  Record.defrecordp(:group_state, :state, Record.extract(:state, from_lib: "kernel/src/group.erl"))

  @history_lines 1000
  # Printed text past this many bytes is drawn and written a slice at a time (`Term.slices/3`).
  @slice 65_536
  # The session's own key, Ctrl+\: the interrupt no screen can take.
  @session_key 0x1C
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
  - `:size`: a function giving the terminal's size as `{cols, rows}`, or `:unknown`; the
    console's by default. It is asked at each prompt. An unknown size is laid out as 80 by 24,
    and a screen asking whether the size is known (`Redoubt.Screen.terminal_size/0`) is told no.
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
    console? = Keyword.get(opts, :input, :console) == :console
    if console?, do: :ok = apply(:beamlet, :console_subscribe, [])

    {cols, rows} = geometry(size)
    group = :group.start(self(), shell, echo: true, expand_below: true, expand_fun: &expand/1)

    state = %{
      group: group,
      term: Term.new(cols, rows),
      output: output,
      size: size,
      history: Keyword.get(opts, :history_lines, @history_lines),
      # Whether the first prompt is still to be drawn on the console: the VM is told when it is
      # (`:beamlet.prompt_drawn/0`), the moment a boot profile times to.
      first_prompt: console?,
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
      # (newest first), bytes that may begin a key, the timer that ends their wait, and the
      # bytes that interrupt it.
      screen: nil,
      # The process fed the lines typed, if any: its pid, its monitor, the line typed so far, and
      # whether the last key was a CR (so that an LF after it ends no second line).
      feed: nil,
      # The line being evaluated, if any: the shell's pid, the token it named it by, and its
      # monitor.
      evaluating: nil,
      esc_timeout: Keyword.get(opts, :esc_timeout, @esc_timeout)
    }

    log = Redoubt.Shell.Log.install(self(), group)

    try do
      loop(state)
    after
      Redoubt.Shell.Log.uninstall(log)
    end
  end

  @doc """
  The driver under the group leader, if the group leader is the shell's own console (OTP's
  `group`, which has completion among its options); `nil` for any other output, a captured one or
  a file, which has no driver to ask.
  """
  @spec of_group() :: pid() | nil
  def of_group do
    group = Process.group_leader()

    with opts when is_list(opts) <- :io.getopts(group),
         true <- Keyword.has_key?(opts, :expand_fun) do
      send(group, {:driver_id, self()})

      receive do
        {^group, :driver_id, driver} -> driver
      after
        1000 -> nil
      end
    else
      _not_a_terminal -> nil
    end
  end

  defp loop(%{group: group} = state) do
    receive do
      {:beamlet_console, input} when state.screen != nil ->
        loop(screen_input(state, input))

      {:beamlet_console, input} ->
        loop(input(state, input))

      {:beamlet_console_resize, {cols, rows}} when is_integer(cols) and is_integer(rows) ->
        loop(resized(state, cols, rows))

      {:redoubt_screen, :open, pid, ctrl_c} when ctrl_c in [:interrupt, :key] ->
        loop(open_screen(state, pid, ctrl_c))

      {:redoubt_feed, :open, pid} ->
        loop(open_feed(state, pid))

      {:DOWN, ref, :process, _pid, _reason} when state.feed != nil and state.feed.ref == ref ->
        loop(close_feed(state))

      {:redoubt_eval, :running, pid, token} ->
        loop(evaluating(state, pid, token))

      {:redoubt_eval, :done, pid, token} ->
        loop(evaluated(state, pid, token))

      {:DOWN, ref, :process, _pid, _reason} when state.evaluating != nil and state.evaluating.ref == ref ->
        loop(%{state | evaluating: nil})

      {:redoubt_screen, :size, pid} ->
        send(pid, {:redoubt_screen, :size, state.size.()})
        loop(state)

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
        {cols, rows} = geometry(state.size)
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

      # clear(): the screen cleared and the cursor home, or, under a screen, once it ends.
      {:redoubt_clear, pid, ref} ->
        state = draw(state, :clear)
        send(pid, {:redoubt_cleared, ref})
        loop(state)

      # The fixed line for a log event the logger's relay would not write through group.
      {:redoubt_shell_log, line} ->
        loop(draw(state, {:put_chars, :unicode, line}))

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
  # the keys typed just before the interrupt, which group was still editing when the driver drew the
  # ^C. They are not drawn.
  defp draw(%{dropping: true} = state, request) do
    if prompting?(request), do: draw(%{state | dropping: false}, request), else: state
  end

  defp draw(state, request) do
    prompting = prompting?(request)
    if prompting, do: trim_history(state)
    term = if prompting, do: sized(state), else: state.term

    term =
      term
      |> Term.slices(request, @slice)
      |> Enum.reduce(term, fn request, term ->
        {out, term} = Term.request(term, request)
        write(state, out)
        term
      end)

    first_prompt = state.first_prompt and not prompting
    if state.first_prompt and prompting, do: :ok = apply(:beamlet, :prompt_drawn, [])
    %{state | term: term, first_prompt: first_prompt}
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
    {cols, rows} = geometry(state.size)
    Term.resize(state.term, cols, rows)
  end

  defp write(state, out) do
    if IO.iodata_length(out) > 0, do: state.output.(out)
    :ok
  end

  # The console's new size is its size from now. A screen in front lays itself out again; with
  # none, the line being edited is drawn again at the new width, and an evaluation's output takes
  # the new width at its next prompt.
  defp resized(state, cols, rows) do
    state = %{state | size: fn -> {cols, rows} end, term: Term.resize(state.term, cols, rows)}

    cond do
      state.screen != nil ->
        send(state.screen.pid, {:resize, cols, rows})
        state

      Term.line_open?(state.term) ->
        {out, term} = Term.request(state.term, :redraw_prompt)
        write(state, out)
        %{state | term: term}

      true ->
        state
    end
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

  # Under a feed, the interrupt is found before the keys are fed: what was typed before it goes
  # to the feed, and what came after goes with the line.
  defp keys(%{feed: %{}} = state, text) do
    case :binary.match(text, [<<3>>, <<@session_key>>]) do
      :nomatch -> feed_keys(state, text)
      {at, 1} -> state |> feed_keys(binary_part(text, 0, at)) |> interrupt_line()
    end
  end

  defp keys(state, text) do
    case :binary.match(text, [<<3>>, <<4>>, <<@session_key>>]) do
      :nomatch ->
        to_group(state, text)

      {at, 1} ->
        <<ahead::binary-size(^at), key, rest::binary>> = text
        key(to_group(state, ahead), key, rest)
    end
  end

  # The interrupt, Ctrl+C or Ctrl+\: the line being edited is dropped and the session stays.
  # What was typed with it in the same read goes with the line, as user_drv drops it too. With
  # no line open, a line is being evaluated, and the interrupt ends it.
  defp key(state, interrupt, _rest) when interrupt in [3, @session_key] do
    if Term.line_open?(state.term) do
      {out, term} = Term.interrupt(state.term)
      write(state, out)
      Process.exit(state.group, :interrupt)
      %{state | term: term, dropping: true}
    else
      interrupt_line(state)
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

  # ---- the line being evaluated ----

  # The shell evaluates a line (`{:redoubt_eval, :running, shell, token}`) until it says it is done;
  # a newer line replaces an older one the driver was not told the end of.
  defp evaluating(state, pid, token) do
    if state.evaluating, do: Process.demonitor(state.evaluating.ref, [:flush])
    %{state | evaluating: %{pid: pid, token: token, ref: Process.monitor(pid)}}
  end

  defp evaluated(%{evaluating: %{pid: pid, token: token, ref: ref}} = state, pid, token) do
    Process.demonitor(ref, [:flush])
    %{state | evaluating: nil}
  end

  defp evaluated(state, _pid, _token), do: state

  # The interrupt with no line being edited: `^C` is drawn, and the line being evaluated, if
  # there is one, is told; with none, the key is dropped. A feed open for the line goes with the
  # line's processes.
  defp interrupt_line(%{evaluating: nil} = state), do: state

  defp interrupt_line(%{evaluating: line} = state) do
    send(line.pid, {:redoubt_interrupt, line.token})
    Process.demonitor(line.ref, [:flush])
    state = %{state | evaluating: nil}

    # A feed's line being typed ends with the ^C after it, as a line being edited does, and the
    # feed goes with the line: what is typed next is the shell's.
    state =
      if Term.line_open?(state.term) do
        {out, term} = Term.interrupt(state.term)
        write(state, out)
        %{state | term: term}
      else
        echo(state, {:put_chars, :unicode, "^C\n"})
      end

    if state.feed, do: close_feed(state), else: state
  end

  # ---- the feed ----

  @doc """
  Takes the lines the person types for the calling process, until it ends: each line arrives as
  `{:redoubt_feed, :data, text}`, and the end of the input, Ctrl+D on an empty line, as
  `{:redoubt_feed, :eof}`. `:none` when the line's output is not the shell's terminal, or a screen
  or another feed holds the keys.
  """
  @spec open_feed() :: :ok | :none
  def open_feed do
    group = Process.group_leader()

    # OTP's group, the shell's own console, has completion among its options; anything else
    # (a captured output, a file) has no driver to ask.
    with opts when is_list(opts) <- :io.getopts(group),
         true <- Keyword.has_key?(opts, :expand_fun),
         driver when is_pid(driver) <- driver_of(group) do
      send(driver, {:redoubt_feed, :open, self()})

      receive do
        {:redoubt_feed, :opened} -> :ok
        {:redoubt_feed, :refused} -> :none
      end
    else
      _not_a_terminal -> :none
    end
  end

  defp driver_of(group) do
    send(group, {:driver_id, self()})

    receive do
      {^group, :driver_id, driver} -> driver
    after
      1000 -> nil
    end
  end

  defp open_feed(%{feed: nil, screen: nil} = state, pid) do
    send(pid, {:redoubt_feed, :opened})
    %{state | feed: %{pid: pid, ref: Process.monitor(pid), line: "", cr: false}}
  end

  defp open_feed(state, pid) do
    send(pid, {:redoubt_feed, :refused})
    state
  end

  # The feed's process has ended: a line it left half typed is ended on the screen, and an empty
  # one taken away, so the shell's next output starts on a line of its own.
  defp close_feed(%{feed: feed} = state) do
    Process.demonitor(feed.ref, [:flush])

    state =
      cond do
        not Term.line_open?(state.term) -> state
        feed.line == "" -> echo(state, :delete_line)
        true -> state |> echo({:insert_chars, :unicode, "\n"}) |> echo(:new_prompt)
      end

    %{state | feed: nil}
  end

  defp feed_keys(state, text), do: text |> String.graphemes() |> Enum.reduce(state, &feed_key(&2, &1))

  # Enter, as CR, LF or both: the line goes to the feed with a newline.
  defp feed_key(%{feed: %{cr: true}} = state, "\n"), do: put_in(state.feed.cr, false)
  defp feed_key(state, enter) when enter in ["\r", "\r\n", "\n"], do: feed_line(state, "\n", enter == "\r")

  # Ctrl+D: on an empty line, the end of the feed's input, and the feed ends; with text on the
  # line, the text goes as it is, without a newline.
  defp feed_key(%{feed: %{line: ""} = feed} = state, <<4>>) do
    send(feed.pid, {:redoubt_feed, :eof})
    close_feed(state)
  end

  defp feed_key(state, <<4>>), do: feed_line(state, "", false)

  defp feed_key(%{feed: %{line: ""}} = state, backspace) when backspace in ["\d", "\b"], do: state

  defp feed_key(state, backspace) when backspace in ["\d", "\b"] do
    state = put_in(state.feed.line, String.slice(state.feed.line, 0..-2//1))
    echo(state, {:delete_chars, -1})
  end

  # Any other control character, the interrupt keys among them, goes nowhere.
  defp feed_key(state, <<c, _::binary>>) when c < 0x20 and c != ?\t, do: state

  defp feed_key(state, key) do
    state = if Term.line_open?(state.term), do: state, else: open_line(state)
    state = %{state | feed: %{state.feed | line: state.feed.line <> key, cr: false}}
    echo(state, {:insert_chars, :unicode, key})
  end

  # A line of the feed's begins as a line with an empty prompt.
  defp open_line(state), do: state |> echo(:new_prompt) |> echo({:insert_chars, :unicode, ""})

  defp feed_line(%{feed: feed} = state, ending, cr) do
    send(feed.pid, {:redoubt_feed, :data, feed.line <> ending})
    state = if Term.line_open?(state.term), do: state, else: open_line(state)
    state = state |> echo({:insert_chars, :unicode, "\n"}) |> echo(:new_prompt)
    %{state | feed: %{feed | line: "", cr: cr}}
  end

  defp echo(state, request) do
    {out, term} = Term.request(state.term, request)
    write(state, out)
    %{state | term: term}
  end

  # ---- a screen in front ----

  # One screen at a time: a second is refused, and its process ends, before it has run any of
  # its module's code. The bytes that interrupt a screen are the session's key, and Ctrl+C
  # unless the screen takes it as a key.
  defp open_screen(%{screen: nil} = state, pid, ctrl_c) do
    {cols, rows} = geometry(state.size)
    write(state, Frame.enter())
    send(pid, {:redoubt_screen, :opened, cols, rows})
    interrupts = if ctrl_c == :key, do: [<<@session_key>>], else: [<<@session_key>>, <<3>>]

    screen = %{
      pid: pid,
      ref: Process.monitor(pid),
      held: [],
      pending: <<>>,
      timer: nil,
      interrupts: interrupts
    }

    %{state | screen: screen}
  end

  defp open_screen(state, pid, _ctrl_c) do
    Process.exit(pid, :another_screen_in_front)
    state
  end

  # A frame the decoder refuses ends the screen: nothing reaches the terminal but cells. It is
  # drawn as it is decoded, a cell at a time, so a whole screen's frame is never one large term.
  defp frame(state, bytes) do
    case Frame.draw_bytes(bytes) do
      {:ok, out} ->
        write(state, out)
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

  # Keys for the screen; the input's end ends it before ending the input. An interrupt byte ends
  # it wherever it falls, found in the bytes before any decoding: no escape sequence or UTF-8
  # sequence holds one, so neither a key the decoder waits on nor a paste can carry it to the
  # screen. The keys before it in the same read reach the screen; those after it go with it.
  defp screen_input(state, :eof), do: state |> interrupt_screen() |> input(:eof)

  defp screen_input(%{screen: screen} = state, bytes) do
    case :binary.match(bytes, screen.interrupts) do
      :nomatch ->
        screen_keys(state, bytes)

      {at, 1} ->
        state |> screen_keys(binary_part(bytes, 0, at)) |> interrupt_screen()
    end
  end

  defp screen_keys(%{screen: nil} = state, _bytes), do: state

  defp screen_keys(%{screen: screen} = state, bytes) do
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
      :unknown -> :unknown
    end
  end

  # The size to lay out at: an unknown one is taken as 80 by 24.
  defp geometry(size) do
    case size.() do
      {cols, rows} -> {cols, rows}
      :unknown -> {80, 24}
    end
  end

  # The shell sets its completer before each read (`Redoubt.Shell.Completer`); until it does, a
  # Tab is a beep.
  defp expand(_before), do: {:no, ~c"", []}
end
