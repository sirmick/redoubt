defmodule Redoubt.Shell.Evaluator do
  @moduledoc """
  Evaluates one line in a process of its own, and prints its result there.

  Each line gets a fresh process, started with a heap limit that kills it, so a runaway
  allocation ends the evaluation instead of taking the session's memory, and a crash, an `exit`
  or a kill ends only that line: the shell keeps the bindings and environment it had before it.
  The result is printed by the evaluator, so printing (reading a large file for a `%Lines{}`, a
  hostile `Inspect`) runs under the same limit.

  **The interrupt** (docs/userland/shell.md, "Interrupting and killing jobs"): while the line runs,
  the shell's driver knows it (`{:redoubt_eval, :running, ...}`), and the interrupt arrives
  here. The line's process is killed with `:kill`, which it cannot trap, and the processes linked
  to it go with it. Every pipeline it ran in the foreground watches it, so their stages' budgets are
  destroyed; the shell waits for that, at most 2 s, before the next prompt, and says so if
  it is still under way.
  """

  alias Redoubt.Shell.Printer

  # How long an interrupted line's foreground jobs are given to end before the next prompt.
  @settle_ms 2_000

  # A fixed share, until the session's budget is known: 16 Mi words, 128 MiB on a 64-bit VM.
  @max_heap_words 16 * 1024 * 1024

  @doc """
  Evaluates `quoted` with `binding` in `env`, printing its value or its error, and returns the
  new binding and environment, `:error` when the line failed and they are unchanged, or `:exit`
  when the line called `exit()`.

  `limits` may set `:max_heap_words`, and `:driver`, the shell's driver, which the interrupt
  comes from (`Redoubt.Shell.Driver.of_group/0`); with none, the line cannot be interrupted.
  """
  @spec eval(Macro.t(), Code.binding(), Macro.Env.t(), keyword()) ::
          {:ok, Code.binding(), Macro.Env.t()} | :error | :exit
  def eval(quoted, binding, env, limits \\ []) do
    shell = self()
    heap = %{size: Keyword.get(limits, :max_heap_words, @max_heap_words), kill: true, error_logger: false}

    {pid, ref} =
      Process.spawn(fn -> evaluate(shell, quoted, binding, env) end, [:monitor, max_heap_size: heap])

    token = make_ref()
    driver = Keyword.get(limits, :driver)
    if driver, do: send(driver, {:redoubt_eval, :running, self(), token})

    try do
      receive do
        {^pid, result} ->
          Process.demonitor(ref, [:flush])
          result

        {:DOWN, ^ref, :process, ^pid, reason} ->
          Printer.text("** (EXIT) the evaluation ended: #{describe(reason)}. The bindings are as they were.")
          :error

        {:redoubt_interrupt, ^token} ->
          interrupted(pid, ref)
      end
    after
      if driver, do: send(driver, {:redoubt_eval, :done, self(), token})

      # An interrupt that crossed the line's end is for no line now.
      receive do
        {:redoubt_interrupt, ^token} -> :ok
      after
        0 -> :ok
      end
    end
  end

  # The line is ended, and the foreground pipelines it ran with it; the driver has drawn the ^C.
  defp interrupted(pid, ref) do
    Process.exit(pid, :kill)

    receive do
      {:DOWN, ^ref, :process, ^pid, _reason} -> :ok
    end

    case Redoubt.Jobs.settle(@settle_ms) do
      :ok ->
        :ok

      {:running, n} ->
        Printer.text("** #{n} of the line's pipelines are still ending: their budgets are being destroyed.")
    end

    :error
  end

  # An exit reason is the ended line's own term, and inspecting it may run the line's own code
  # (an Inspect implementation), so it is inspected in a process of its own, with a heap limit
  # and a time limit, never in the shell's.
  defp describe(reason) do
    heap = %{size: 1024 * 1024, kill: true, error_logger: false}

    {pid, ref} =
      Process.spawn(fn -> exit({:described, inspect(reason, limit: 50)}) end, [:monitor, max_heap_size: heap])

    receive do
      {:DOWN, ^ref, :process, ^pid, {:described, text}} -> text
      {:DOWN, ^ref, :process, ^pid, _other} -> "a reason that could not be shown"
    after
      1000 ->
        Process.exit(pid, :kill)
        "a reason that could not be shown in time"
    end
  end

  defp evaluate(shell, quoted, binding, env) do
    # The compiler's errors and warnings are taken, not left for it to write to the console
    # itself, so they are printed through the printer like everything else.
    {outcome, diagnostics} =
      Code.with_diagnostics([log: false], fn ->
        try do
          {:ok, Code.eval_quoted_with_env(quoted, binding, env)}
        catch
          kind, reason -> {kind, reason, __STACKTRACE__}
        end
      end)

    Enum.each(diagnostics, &Printer.diagnostic/1)

    result =
      case outcome do
        {:ok, {value, binding, env}} ->
          # A value that fails to print still keeps the line's bindings.
          try do
            Printer.value(value)
          catch
            kind, reason -> Printer.error(kind, reason, __STACKTRACE__)
          end

          {:ok, binding, env}

        {:throw, :redoubt_shell_exit, _stack} ->
          :exit

        # The diagnostics said what was wrong; this says only that the line did not compile.
        {:error, %CompileError{}, _stack} when diagnostics != [] ->
          :error

        {kind, reason, stack} ->
          Printer.error(kind, reason, stack)
          :error
      end

    send(shell, {self(), result})
  end
end
