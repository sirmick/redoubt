defmodule Redoubt.Screen do
  @moduledoc """
  A full-screen program (docs/userland/shell.md, "Full-screen programs"): an Erlang process in
  the session's VM, in the Elm style, that draws into a screen buffer of beamlet's natives and
  hands the session's driver what changed.

  A screen implements three functions:
  - `init(args)`: its first state;
  - `update(event, state)`: `{:cont, state}`, or `{:halt, value}` to end with `value`. An event
    is `{:key, key, modifiers}` (`Redoubt.Term.Keys`), `{:resize, cols, rows}` (the first event,
    with the screen's size, and again whenever the terminal changes size), or any other message
    the process gets;
  - `view(state, buffer, {cols, rows})`: draws the whole screen into the buffer, which starts
    blank each time; only what changed since the last frame is sent.

  `run/3`, from a line, runs one: it starts the screen's process, with its caller's heap limit (the
  evaluator's, from a line), and returns the value it ended with, or `nil` when the interrupt ended it. The interrupt is the
  session's key, Ctrl+\\, which no screen is ever sent, and Ctrl+C unless the screen takes it as
  a key. While it is in front, the shell's driver shows the alternate screen, sends it the keys,
  draws its frames through the one decoder and encoder, and holds other processes' output; then
  it puts the screen and the line back as they were and draws what it held.
  """

  alias Redoubt.Term.Buffer

  @callback init(args :: term()) :: term()
  @callback update(event :: term(), state :: term()) :: {:cont, term()} | {:halt, term()}
  @callback view(state :: term(), buffer :: Buffer.t(), size :: {pos_integer(), pos_integer()}) :: term()

  # The screen's process's heap limit when its caller has none: the evaluator's fixed share until
  # the session's budget is known (128 MiB on a 64-bit VM).
  @max_heap_words 16 * 1024 * 1024

  @doc """
  Runs `module` with `args` as a screen, and returns the value its `update` ended it with, or
  `nil` if the interrupt ended it. Raises when there is no terminal to draw on (the line's
  output is not the shell's console).

  Options:
  - `:ctrl_c` (`:interrupt`): with `:key`, Ctrl+C reaches the screen as the key
    `{:key, "c", [:ctrl]}`, and Ctrl+\\ alone ends it.
  """
  @spec run(module(), term(), keyword()) :: term()
  def run(module, args, opts \\ []) do
    {value, nil} = serve(module, args, fn _request, nil -> {nil, nil} end, nil, opts)
    value
  end

  @doc """
  Runs `module` as `run/3` does, and while it is in front answers what it asks with `call/1`:
  `handle.(request, state)` returns the reply and the next state, in the caller's process. So a
  screen can be handed what only its caller may touch, a file the caller opened, a bit at a time.
  Returns the screen's value and the last state.
  """
  @spec serve(module(), term(), (term(), state -> {term(), state}), state, keyword()) :: {term(), state}
        when state: term()
  def serve(module, args, handle, state, opts \\ []) do
    ctrl_c = Keyword.get(opts, :ctrl_c, :interrupt)

    unless ctrl_c in [:interrupt, :key],
      do: raise(ArgumentError, "ctrl_c is :interrupt or :key, not #{inspect(ctrl_c)}")

    driver = driver!()
    caller = self()
    # What a screen holds is what it shows (the pager keeps the lines it has read), and lines are
    # binaries, which a heap limit leaves out unless told: so they count toward this one.
    heap = %{size: heap_words(), kill: true, error_logger: false, include_shared_binaries: true}

    {pid, ref} =
      Process.spawn(fn -> start(driver, caller, module, args, ctrl_c) end, [:monitor, max_heap_size: heap])

    await(pid, ref, handle, state)
  end

  defp await(pid, ref, handle, state) do
    receive do
      {:redoubt_screen, :call, ^pid, tag, request} ->
        {reply, state} = handle.(request, state)
        send(pid, {tag, reply})
        await(pid, ref, handle, state)

      {:redoubt_screen, :result, ^pid, value} ->
        Process.demonitor(ref, [:flush])
        {value, state}

      {:DOWN, ^ref, :process, ^pid, :interrupt} ->
        {nil, state}

      {:DOWN, ^ref, :process, ^pid, reason} ->
        exit({:screen, reason})
    end
  end

  @doc """
  From a screen's process: asks the process that runs it (`serve/5`), and waits for the reply.
  If that process ends first, so does the screen.
  """
  @spec call(term()) :: term()
  def call(request) do
    caller = Process.get(:redoubt_screen_caller)
    tag = Process.monitor(caller)
    send(caller, {:redoubt_screen, :call, self(), tag, request})

    receive do
      {^tag, reply} ->
        Process.demonitor(tag, [:flush])
        reply

      {:DOWN, ^tag, :process, _caller, reason} ->
        exit({:caller, reason})
    end
  end

  @doc """
  The size of the terminal a screen would draw on, `{:ok, {cols, rows}}`, or `:none` when the
  line's output is not the shell's terminal, or the terminal's size is not known (a console that
  does not say it), when a screen's layout would be a guess.
  """
  @spec terminal_size() :: {:ok, {pos_integer(), pos_integer()}} | :none
  def terminal_size do
    case Redoubt.Shell.Driver.of_group() do
      nil ->
        :none

      driver ->
        send(driver, {:redoubt_screen, :size, self()})

        receive do
          {:redoubt_screen, :size, {cols, rows}} -> {:ok, {cols, rows}}
          {:redoubt_screen, :size, :unknown} -> :none
        end
    end
  end

  # The caller's own limit, so a screen holds no more than the line that started it could.
  defp heap_words do
    case Process.info(self(), :max_heap_size) do
      {:max_heap_size, %{size: words}} when words > 0 -> words
      _none -> @max_heap_words
    end
  end

  # The shell's driver: the group leader, OTP's group, knows it.
  defp driver! do
    Redoubt.Shell.Driver.of_group() ||
      raise ArgumentError, "a screen needs the shell's terminal: this line's output is not it"
  end

  # ---- the screen's process ----

  defp start(driver, caller, module, args, ctrl_c) do
    Process.put(:redoubt_screen_caller, caller)
    send(driver, {:redoubt_screen, :open, self(), ctrl_c})

    {cols, rows} =
      receive do
        {:redoubt_screen, :opened, cols, rows} -> {cols, rows}
      after
        5000 -> exit(:no_terminal)
      end

    buffer = Buffer.new(cols, rows)
    screen = %{driver: driver, caller: caller, module: module, buffer: buffer, size: {cols, rows}}
    handle(screen, module.init(args), {:resize, cols, rows})
  end

  # A change of the terminal's size makes the buffer that size, blank, before the screen lays
  # itself out again: the next frame clears the terminal and sends it all.
  defp loop(screen, state) do
    receive do
      {:resize, cols, rows} = event when is_integer(cols) and is_integer(rows) ->
        Buffer.resize(screen.buffer, cols, rows)
        handle(%{screen | size: {cols, rows}}, state, event)

      event ->
        handle(screen, state, event)
    end
  end

  defp handle(screen, state, event) do
    case screen.module.update(event, state) do
      {:cont, state} ->
        draw(screen, state)
        loop(screen, state)

      {:halt, value} ->
        send(screen.driver, {:redoubt_screen, :close, self()})
        send(screen.caller, {:redoubt_screen, :result, self(), value})
    end
  end

  # The whole screen drawn afresh into a blank buffer; the diff is what changed.
  defp draw(%{buffer: buffer, size: {cols, rows}} = screen, state) do
    Buffer.fill(buffer, {0, 0, cols, rows}, " ")
    screen.module.view(state, buffer, screen.size)
    send(screen.driver, {:redoubt_screen, :frame, self(), Buffer.diff(buffer)})
  end
end
