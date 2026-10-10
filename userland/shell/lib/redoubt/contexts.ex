defmodule Redoubt.Contexts do
  @moduledoc """
  The session's contexts (docs/userland/sessions.md, "Contexts"), through the steward's generated
  client on the session's own `steward` connection: the steward answers from the session's
  domain, its principal and label set as the kernel stamped them, never from anything the session
  says. It lists that domain's contexts, lets the session's own channel go, and ends a context of
  that domain by name; another label set's or principal's is `:unknown`, as a name nobody holds.
  """

  alias Redoubt.Wire.Client.Steward

  @doc "The session's connection to the steward, or `{:error, :no_steward}` where it has none."
  @spec connection() :: {:ok, reference()} | {:error, :no_steward}
  def connection do
    case Redoubt.Namespace.handle("steward") do
      {:ok, steward} -> {:ok, steward}
      {:error, _} -> {:error, :no_steward}
    end
  end

  @doc "The live contexts of the session's domain, by name."
  @spec list() :: {:ok, [map()]} | {:error, atom()}
  def list do
    with {:ok, steward} <- connection(), {:ok, %{list: text}} <- Steward.contexts(steward) do
      {:ok, parse(text)}
    end
  end

  @doc "Lets the session's own channel go; the context runs on, detached."
  @spec detach() :: :ok | {:error, atom()}
  def detach do
    with {:ok, steward} <- connection(), {:ok, _} <- Steward.detach(steward), do: :ok
  end

  @doc "Ends the context `name` of the session's domain, the session's own included."
  @spec end_context(String.t()) :: :ok | {:error, atom()}
  def end_context(name) when is_binary(name) do
    with {:ok, steward} <- connection(), {:ok, _} <- Steward.end_context(steward, name), do: :ok
  end

  @doc """
  The steward's listing, one line per context, `name<TAB>attached|detached<TAB>seconds`, as maps:
  the name (empty for the default context), its state, and the seconds since it last started
  running, attached or detached. A line in any other form is left out.
  """
  @spec parse(String.t()) :: [map()]
  def parse(text) when is_binary(text) do
    text
    |> String.split("\n", trim: true)
    |> Enum.flat_map(fn line ->
      with [name, state, secs] <- String.split(line, "\t"),
           {:ok, state} <- state(state),
           {age, ""} <- Integer.parse(secs) do
        [%{name: name, state: state, age: age}]
      else
        _ -> []
      end
    end)
  end

  # Named here, so the atoms exist in the boot pack whatever else it holds.
  defp state("attached"), do: {:ok, :attached}
  defp state("detached"), do: {:ok, :detached}
  defp state(_), do: :error
end
