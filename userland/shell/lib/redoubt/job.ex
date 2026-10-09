defmodule Redoubt.Job do
  @moduledoc """
  A job: one pipeline of native stages, each in a budget of its own carved from the session's
  (docs/userland/shell.md, "Interrupting and killing jobs"). There are no signals: killing a job
  is destroying its stages' budgets, which ends every process in them and nothing else, never the
  session.

  `start/2` runs a pipeline in the background: it reads no console, it is not ended by the
  interrupt or by the line that started it, and what it writes is kept, bounded, until `await/2`
  takes it. `pipe/1` and `exec` run theirs in the foreground, and the interrupt ends them with
  their line.
  """

  alias Redoubt.{Jobs, Pipeline}

  @enforce_keys [:id, :command]
  defstruct [:id, :command]

  @type t :: %__MODULE__{id: pos_integer(), command: String.t()}

  @doc """
  Starts `stages` (`Redoubt.Pipeline.stages/1` gives them from words) in the background, and
  returns the job at once, or `{:error, name}` when it cannot start. Options:
  - `:input`: the lines the first stage reads; with none, the end of its input at once. Never
    the console: a background job cannot take what is typed.
  - `:budget`: each stage's budget spec, as `Redoubt.Pipeline.run/2` takes it.

  What the last stage writes, and every stage's standard error, is kept up to 64 KiB each; what
  comes past that is dropped and counted.
  """
  @spec start([Pipeline.stage()], keyword()) :: {:ok, t()} | {:error, atom()}
  def start(stages, opts \\ []) do
    if Keyword.get(opts, :input) == :console,
      do: {:error, :console},
      else: Pipeline.start(stages, Keyword.take(opts, [:input, :budget]))
  end

  @doc "`:running`, or how the job ended: `:exited`, `:faulted`, `:killed`, or `:unknown`."
  @spec status(t()) :: Jobs.state() | :unknown
  def status(%__MODULE__{id: id}), do: Jobs.status(id)

  @doc """
  Ends the job: every stage's budget is destroyed, and with it everything the stage started. Gives
  the job's result, as `await/2` does; a job that had already ended is not touched.
  """
  @spec kill(t()) :: map() | {:error, :unknown}
  def kill(%__MODULE__{id: id}), do: Jobs.kill(id)

  @doc """
  Waits at most `timeout` for the job to end, and gives `%{stdout:, stderr:, endings:, dropped:}`,
  `dropped` being the bytes past each bound, `{stdout, stderr}`; `:timeout`; or `{:error,
  :unknown}` for a job whose result was taken.
  """
  @spec await(t(), timeout()) :: map() | :timeout | {:error, :unknown}
  def await(%__MODULE__{id: id}, timeout \\ :infinity), do: Jobs.await(id, timeout)

  @doc "The session's jobs, as `Redoubt.Jobs.list/0` gives them."
  @spec list() :: [map()]
  def list, do: Jobs.list()
end
