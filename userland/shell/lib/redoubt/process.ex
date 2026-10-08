defmodule Redoubt.Process do
  @moduledoc """
  Native programs (docs/userland/native.md, "Launching from a session"), over beamlet's launch
  native: every authority a program gets is on the one call, from the caller, and the session's
  own handles are never passed on; the program's end comes back as a message.

  `run/3` runs one program as a pipeline of one stage (`Redoubt.Pipeline`): its standard input the
  lines the person types, its output and standard error drawn on the console through the shell's
  guard. It never holds the console itself.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  @typedoc "How a job ended: `{:exited, code}`, `{:faulted, cause}` or `{:killed, 0}`."
  @type ending :: {:exited | :faulted | :killed | :ended, non_neg_integer()}

  @doc """
  Starts a program from `launch`: `%{image: bytes, budget: budget}`, and `namespace: [{path,
  connection}]`, `handles: [{name, handle}]`, `args: [string]`, `stack_pages:` and `heap_pages:` as
  it needs. Its end arrives as `{:exit, job, cause, code}`.
  """
  @spec launch(map()) :: {:ok, reference()} | {:error, atom()}
  def launch(launch) when is_map(launch), do: :redoubt.launch(launch)

  @doc "Waits for `job`'s end."
  @spec await(reference(), timeout()) :: ending() | :timeout
  def await(job, timeout \\ :infinity) do
    receive do
      {:exit, ^job, cause, code} -> {cause, code}
    after
      timeout -> :timeout
    end
  end

  @doc """
  Runs the program `/boot/NAME` with `args`, in a budget carved from the session's (`budget:`, a
  spec, as `Redoubt.Pipeline.run/2` takes it), reading the lines the person types and drawing what
  it writes, and waits for its end: `{:ok, ending, usage}`, the budget's use read as the program
  ended, or `{:error, name}`. The budget is destroyed before it returns.
  """
  @spec run(String.t(), [String.t()], keyword()) :: {:ok, ending(), map() | nil} | {:error, atom()}
  def run(name, args \\ [], opts \\ []) do
    opts = Keyword.merge([input: :console, output: :console], Keyword.take(opts, [:budget, :input]))

    case Redoubt.Pipeline.run([{name, args}], opts) do
      {:ok, %{endings: [ending], usage: [usage]}} -> {:ok, ending, usage}
      {:error, _} = error -> error
    end
  end
end
