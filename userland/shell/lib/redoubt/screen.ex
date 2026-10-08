defmodule Redoubt.Screen do
  @moduledoc """
  A full-screen program (docs/userland/shell.md, "Full-screen programs"): an Erlang process in
  the session's VM, in the Elm style, that draws into a screen buffer of beamlet's natives and
  hands the session's driver what changed.

  A screen implements three functions:
  - `init(args)`: its first state;
  - `update(event, state)`: `{:cont, state}`, or `{:halt, value}` to end with `value`. An event
    is `{:key, key, modifiers}` (`Redoubt.Term.Keys`), `{:resize, cols, rows}` (the first event,
    with the screen's size), or any other message the process gets;
  - `view(state, buffer, {cols, rows})`: draws the whole screen into the buffer, which starts
    blank each time; only what changed since the last frame is sent.

  `run/2`, from a line, runs one: it starts the screen's process, with the evaluator's heap limit,
  and returns the value it ended with, or `nil` when the interrupt ended it. While it is in front,
  the shell's driver shows the alternate screen, sends it the keys, draws its frames through the
  one decoder and encoder, and holds other processes' output; then it puts the screen and the
  line back as they were and draws what it held.
  """

  alias Redoubt.Term.Buffer

  @callback init(args :: term()) :: term()
  @callback update(event :: term(), state :: term()) :: {:cont, term()} | {:halt, term()}
  @callback view(state :: term(), buffer :: Buffer.t(), size :: {pos_integer(), pos_integer()}) :: term()

  # The screen's process's heap limit, as the evaluator's: a fixed share until the session's
  # budget is known (128 MiB on a 64-bit VM).
  @max_heap_words 16 * 1024 * 1024

  @doc """
  Runs `module` with `args` as a screen, and returns the value its `update` ended it with, or
  `nil` if the interrupt ended it. Raises when there is no terminal to draw on (the line's
  output is not the shell's console).
  """
  @spec run(module(), term()) :: term()
  def run(module, args) do
    driver = driver!()
    caller = self()
    heap = %{size: @max_heap_words, kill: true, error_logger: false}

    {pid, ref} =
      Process.spawn(fn -> start(driver, caller, module, args) end, [:monitor, max_heap_size: heap])

    receive do
      {:redoubt_screen, :result, ^pid, value} ->
        Process.demonitor(ref, [:flush])
        value

      {:DOWN, ^ref, :process, ^pid, :interrupt} ->
        nil

      {:DOWN, ^ref, :process, ^pid, reason} ->
        exit({:screen, reason})
    end
  end

  # The shell's driver: the group leader, OTP's group, knows it.
  defp driver! do
    group = Process.group_leader()
    send(group, {:driver_id, self()})

    receive do
      {^group, :driver_id, driver} -> driver
    after
      1000 -> raise ArgumentError, "a screen needs the shell's terminal: this line's output is not it"
    end
  end

  # ---- the screen's process ----

  defp start(driver, caller, module, args) do
    send(driver, {:redoubt_screen, :open, self()})

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

  defp loop(screen, state), do: handle(screen, state, receive(do: (event -> event)))

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
