defmodule Redoubt.Process do
  @moduledoc """
  Native programs (docs/userland/native.md, "Launching from a session"), over beamlet's launch
  native: every authority a program gets is on the one call, from the caller, and the session's
  own handles are never passed on; the program's end comes back as a message.

  `run/3` is the whole of a launch: the program's bytes read from `/boot`, a budget carved from
  the session's, a fresh connection to the console as the program's `/dev/cons`, and the wait for
  its end, after which the console connection is let go and the budget destroyed.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  alias Redoubt.Budget
  alias Redoubt.Wire.Client.NinepCommon

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

  @default_budget %{pages: 256, processes: 1, weight: 1}

  @doc """
  Runs the program `/boot/NAME` with `args`, in a budget carved from the session's (`budget:`, a
  spec, `#{inspect(@default_budget)}` when left out), with the session's console as its
  `/dev/cons`, and waits for its end: `{:ok, ending, usage}`, the budget's use read as the program
  ended, or `{:error, name}`.
  """
  @spec run(String.t(), [String.t()], keyword()) :: {:ok, ending(), map()} | {:error, atom()}
  def run(name, args \\ [], opts \\ []) do
    with {:ok, image} <- read("/boot/" <> name),
         {:ok, console, _} <- Redoubt.Namespace.lookup("/dev/cons"),
         {:ok, budget} <- Budget.carve(Keyword.get(opts, :budget, @default_budget)) do
      try do
        launched(image, budget, console, args)
      after
        Budget.destroy(budget)
      end
    end
  end

  defp launched(image, budget, console, args) do
    with {:ok, %{conn: conn, id: id}} <- NinepCommon.new_connection(console, "", 0) do
      try do
        launch = %{image: image, budget: budget, namespace: [{"/dev/cons", conn}], args: args}

        with {:ok, job} <- launch(launch) do
          ending = await(job)
          {:ok, usage} = Budget.usage(budget)
          {:ok, ending, usage}
        end
      after
        NinepCommon.disconnect(console, id)
      end
    end
  end

  defp read(path) do
    case File.read(path) do
      {:ok, bytes} -> {:ok, bytes}
      {:error, :enoent} -> {:error, :not_found}
      {:error, _} -> {:error, :refused}
    end
  end
end
