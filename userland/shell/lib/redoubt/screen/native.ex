defmodule Redoubt.Screen.Native do
  @moduledoc """
  A native program's screen (docs/userland/shell.md, "A native program's screen and the
  session's key"): a program from `/boot`, run as a one-stage pipeline in a budget of its own,
  that draws by sending `cells` frames on its standard output and reads its keys and size as
  events on its standard input.

  The line asks for it (`run/3`, the shell's `screen`); the program never chooses how its output
  is read. While it runs, one Erlang process, the **host**, is the screen in front for the shell's
  driver, as a `Redoubt.Screen`'s process is. The host:
  - reads the program's output as records (`Redoubt.Term.Cells.split/2`), refusing one whose
    length is more than a frame of its screen can take before holding any of it;
  - decodes each through the one decoder, a cell at a time (`Redoubt.Term.Cells.reduce/3`), so
    a whole screen's frame is never one large term; and refuses a frame the decoder refuses, a
    frame of any size but the one it gave the program (a frame of a size it gave before the
    terminal last changed size is dropped, not drawn), and a cell that is not one cell's worth as
    the session lays it out: one grapheme, and a wide one not in the last column;
  - puts each cell into a screen buffer of its own as it is read, and hands the driver the buffer's diff at
    most 30 times a second, which the driver draws through the one encoder;
  - asks for the program's output at most 1 MiB a second: past that its read
    waits, the pipe fills, and the program's write waits;
  - sends the program its size first and again at each change, and each key the driver sends,
    never the session's key (the event encoder refuses it); keys past 64 KiB of events not yet
    written into the program's input pipe, which holds a page more, are dropped.

  A refusal ends the program at once, its budget destroyed, and the screen; nothing of the
  refused record is drawn. The interrupt (Ctrl+\\, and Ctrl+C unless `ctrl_c: :key`) ends the
  host, which ends the program's pipeline with it, as an interrupted line's pipelines end. The
  program's standard error is kept, 64 KiB of it, and drawn as the line's own output after
  the screen ends, through the shell's guard.
  """

  alias Redoubt.Pipeline
  alias Redoubt.Term.{Buffer, Cells, Width}

  @bytes_per_second 1024 * 1024
  # One frame drawn at most every this many milliseconds: 30 a second.
  @tick_ms 33
  @max_unread 64 * 1024
  # How long an ended or refused program's budget is given to be destroyed, as an interrupted
  # line's are.
  @settle_ms 2_000
  # Sizes given before the last change of size, whose frames are dropped while still arriving.
  @stale_sizes 4
  @max_heap_words 16 * 1024 * 1024

  @doc """
  Runs `/boot/name` with `args` as a screen, and returns how it ended: `{:exited, code}`,
  `{:faulted, cause}` or `{:killed, 0}`; `{:error, {:refused, why}}` when the session refused
  what it sent (`why` one of `Redoubt.Term.Cells`' errors, or `:size` or `:cell`); `{:error,
  name}` when it could not start; or `nil` when the interrupt ended it. Raises when the line's
  output is not the shell's terminal.

  Options: `:ctrl_c` (`:interrupt`), as `Redoubt.Screen.run/3`'s: with `:key`, Ctrl+C reaches
  the program as a key, and Ctrl+\\ alone ends it. The tests also give `:start`, a function
  standing for the pipeline, called with the host, and `:bytes_per_second` and `:tick_ms`.
  """
  @spec run(String.t(), [String.t()], keyword()) :: term()
  def run(name, args \\ [], opts \\ []) do
    ctrl_c = Keyword.get(opts, :ctrl_c, :interrupt)

    unless ctrl_c in [:interrupt, :key],
      do: raise(ArgumentError, "ctrl_c is :interrupt or :key, not #{inspect(ctrl_c)}")

    driver =
      Redoubt.Shell.Driver.of_group() ||
        raise ArgumentError, "a screen needs the shell's terminal: this line's output is not it"

    config = %{
      driver: driver,
      caller: self(),
      ctrl_c: ctrl_c,
      start: Keyword.get(opts, :start, fn host -> pipeline(name, args, host) end),
      bytes_per_second: Keyword.get(opts, :bytes_per_second, @bytes_per_second),
      tick_ms: Keyword.get(opts, :tick_ms, @tick_ms)
    }

    heap = %{size: heap_words(), kill: true, error_logger: false, include_shared_binaries: true}
    {pid, ref} = Process.spawn(fn -> host(config) end, [:monitor, max_heap_size: heap])
    await(pid, ref)
  end

  defp pipeline(name, args, host),
    do: Pipeline.run([{name, args}], input: {:records, host}, output: {:records, host})

  defp await(pid, ref) do
    receive do
      {__MODULE__, :result, ^pid, value, errors} ->
        Process.demonitor(ref, [:flush])
        settle(value)
        show(errors)
        value

      {:DOWN, ^ref, :process, ^pid, :interrupt} ->
        settle(nil)
        nil

      {:DOWN, ^ref, :process, ^pid, reason} ->
        exit({:screen, reason})
    end
  end

  # An ended program's budget was destroyed before its pipeline returned; a refused or
  # interrupted one's is being destroyed, and the line waits for it, as after an interrupt.
  defp settle({:exited, _code}), do: :ok
  defp settle({:faulted, _cause}), do: :ok
  defp settle({:killed, _code}), do: :ok

  defp settle(_refused_or_interrupted) do
    with {:running, _n} <- Redoubt.Jobs.settle(@settle_ms),
         do: IO.puts("(the screen's program is still ending)")
  end

  # The program's standard error, as the line's own output, so through the shell's guard.
  defp show(nil), do: :ok

  defp show({bytes, dropped}) do
    if bytes != "", do: IO.write(text(bytes))
    if dropped > 0, do: IO.puts("(#{dropped} more bytes of standard error not kept)")
  end

  # A byte that is not UTF-8 is the Latin-1 character it is, as a stage's drawn output's is.
  defp text(bytes) do
    case :unicode.characters_to_binary(bytes) do
      text when is_binary(text) -> text
      {_error_or_incomplete, text, <<byte, rest::binary>>} -> text <> <<byte::utf8>> <> text(rest)
    end
  end

  defp heap_words do
    case Process.info(self(), :max_heap_size) do
      {:max_heap_size, %{size: words}} when words > 0 -> words
      _none -> @max_heap_words
    end
  end

  # ---- the host ----

  defp host(config) do
    send(config.driver, {:redoubt_screen, :open, self(), config.ctrl_c})

    {cols, rows} =
      receive do
        {:redoubt_screen, :opened, cols, rows} -> {cols, rows}
      after
        5000 -> exit(:no_terminal)
      end

    me = self()
    runner = spawn_link(fn -> send(me, {__MODULE__, :ended, config.start.(me)}) end)

    state = %{
      config: config,
      runner: runner,
      buffer: Buffer.new(cols, rows),
      size: {cols, rows},
      stale: [],
      pending: <<>>,
      feeder: nil,
      queued: [],
      unread: 0,
      reader: nil,
      window: now(),
      allowance: config.bytes_per_second,
      dirty: false,
      tick: false
    }

    state |> event({:size, cols, rows}) |> loop()
  end

  defp loop(state) do
    receive do
      {:key, _key, _modifiers} = key ->
        loop(event(state, key))

      {:resize, cols, rows} when is_integer(cols) and is_integer(rows) ->
        Buffer.resize(state.buffer, cols, rows)
        stale = Enum.take([state.size | state.stale] -- [{cols, rows}], @stale_sizes)
        %{state | size: {cols, rows}, stale: stale} |> dirty() |> event({:size, cols, rows}) |> loop()

      {Pipeline, :stdin, feeder} ->
        state.queued |> Enum.reverse() |> Enum.each(&send(feeder, {:write, &1}))
        loop(%{state | feeder: feeder, queued: []})

      {Pipeline, :written, n} ->
        loop(%{state | unread: state.unread - n})

      {Pipeline, :stdout, reader, bytes} ->
        state |> output(bytes) |> pace(reader, byte_size(bytes)) |> loop()

      {__MODULE__, :second} ->
        state = %{state | window: now(), allowance: state.config.bytes_per_second}
        if state.reader, do: send(state.reader, {Pipeline, :more})
        loop(%{state | reader: nil})

      {__MODULE__, :tick} ->
        loop(draw(%{state | tick: false}))

      {__MODULE__, :ended, result} ->
        finish(state, ending(result), errors(result))

      _other ->
        loop(state)
    end
  end

  # An event for the program, sent once its input is open; dropped while 64 KiB of events are
  # not yet written into its pipe (the feeder answers each write once the pipe has taken it).
  defp event(state, event) do
    # The encoder refuses the interrupt keys: the driver finds them before decoding any key, and
    # the encoder is the boundary that holds even if it did not.
    case Cells.event(event, ctrl_c: state.config.ctrl_c) do
      {:ok, body} ->
        bytes = Cells.record(body)

        cond do
          state.unread + byte_size(bytes) > @max_unread ->
            state

          state.feeder ->
            send(state.feeder, {:write, bytes})
            %{state | unread: state.unread + byte_size(bytes)}

          true ->
            %{state | queued: [bytes | state.queued], unread: state.unread + byte_size(bytes)}
        end

      {:error, _not_an_event} ->
        state
    end
  end

  # The program's output, record by record, each refused or drawn as it completes.
  defp output(state, bytes), do: records(%{state | pending: state.pending <> bytes})

  defp records(state) do
    case Cells.split(state.pending, bound(state)) do
      :more -> state
      {:error, why} -> refuse(state, why)
      {:ok, body, rest} -> %{state | pending: rest} |> frame(body) |> records()
    end
  end

  # The longest record a frame of any size still awaited may take.
  defp bound(state),
    do: [state.size | state.stale] |> Enum.map(fn {c, r} -> Cells.max_frame(c, r) end) |> Enum.max()

  # A frame is checked and put into the buffer a cell at a time, as the one decoder reads it, so a
  # whole screen's frame is never held as one term. A frame refused after some of its cells were
  # put draws none of them: the screen ends, and the buffer's diff is never sent.
  defp frame(%{buffer: buffer, size: {cols, rows}} = state, body) do
    step = fn
      {:frame, %{width: ^cols, height: ^rows, clear: clear}}, nil ->
        if clear, do: Buffer.fill(buffer, {0, 0, cols, rows}, " ")
        {:cont, nil}

      # A frame of a size given before the last change is dropped; of any other, refused.
      {:frame, %{width: w, height: h}}, nil ->
        {:halt, if({w, h} in state.stale, do: :stale, else: {:refused, :size})}

      {:cell, cell}, nil ->
        if one_cell?(cell, cols) do
          Buffer.put(buffer, cell.x, cell.y, cell.symbol, {cell.fg, cell.bg, cell.modifiers})
          {:cont, nil}
        else
          {:halt, {:refused, :cell}}
        end
    end

    case Cells.reduce(body, nil, step) do
      # The size given is drawn, and frames of earlier sizes are no longer awaited.
      {:ok, nil} -> dirty(%{state | stale: []})
      {:halt, :stale} -> state
      {:halt, {:refused, why}} -> refuse(state, why)
      {:error, why} -> refuse(state, why)
    end
  end

  # One cell's worth: one grapheme, and a wide one with room for it.
  defp one_cell?(%{symbol: symbol, x: x}, cols) do
    case String.graphemes(symbol) do
      [^symbol] -> Width.grapheme(symbol) == 1 or x + 1 < cols
      _more -> false
    end
  end

  # What changed goes to the driver at the next tick, so at most one frame a tick.
  defp dirty(%{tick: true} = state), do: %{state | dirty: true}

  defp dirty(state) do
    Process.send_after(self(), {__MODULE__, :tick}, state.config.tick_ms)
    %{state | dirty: true, tick: true}
  end

  defp draw(%{dirty: false} = state), do: state

  defp draw(state) do
    send(state.config.driver, {:redoubt_screen, :frame, self(), Buffer.diff(state.buffer)})
    %{state | dirty: false}
  end

  # The next read is asked for while this second's bytes last; past them, at the next second.
  defp pace(state, reader, n) do
    state =
      if now() - state.window >= 1000,
        do: %{state | window: now(), allowance: state.config.bytes_per_second},
        else: state

    state = %{state | allowance: state.allowance - n}

    if state.allowance > 0 do
      send(reader, {Pipeline, :more})
      state
    else
      Process.send_after(self(), {__MODULE__, :second}, max(state.window + 1000 - now(), 0))
      %{state | reader: reader}
    end
  end

  # A refusal: the program's pipeline ends at once, its budget destroyed by its owner, and the
  # screen with it. Nothing of the record is drawn.
  defp refuse(state, why) do
    Process.unlink(state.runner)
    Process.exit(state.runner, :kill)
    finish(state, {:error, {:refused, why}}, nil)
  end

  defp finish(state, value, errors) do
    send(state.config.driver, {:redoubt_screen, :close, self()})
    send(state.config.caller, {__MODULE__, :result, self(), value, errors})
    exit(:normal)
  end

  defp ending({:ok, %{endings: [ending]}}), do: ending
  defp ending({:error, _name} = error), do: error

  defp errors({:ok, %{errors: errors}}), do: errors
  defp errors(_error), do: nil

  defp now, do: System.monotonic_time(:millisecond)
end
