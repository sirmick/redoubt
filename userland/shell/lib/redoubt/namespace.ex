defmodule Redoubt.Namespace do
  @moduledoc """
  The session's namespace (docs/userland/sessions.md, "Namespaces"), over beamlet's natives: a
  table of path prefixes, each naming a connection the session holds, and the named handles it
  was started with. A thin layer: it adds no authority, and every refusal is the platform's, by a
  Redoubt name.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  @doc """
  The connection the longest matching prefix of `path` names, and the rest of the path below it;
  for a name with no `/`, the named handle of that name (`"keyd"`, `"budget"`) and `""`.
  """
  @spec lookup(String.t()) :: {:ok, reference(), String.t()} | {:error, atom()}
  def lookup(path) when is_binary(path), do: :redoubt.ns_lookup(path)

  @doc "The handle named `name`."
  @spec handle(String.t()) :: {:ok, reference()} | {:error, atom()}
  def handle(name) when is_binary(name) do
    with {:ok, handle, _} <- lookup(name), do: {:ok, handle}
  end

  @doc """
  Puts `connection`, one the session holds, at `prefix` as well: one connection, one badge, under
  two names. It creates no authority.
  """
  @spec bind(String.t(), reference()) :: :ok | {:error, atom()}
  def bind(prefix, connection) when is_binary(prefix), do: :redoubt.bind(prefix, connection)

  @doc "The table: `{path, name, handle}`, the namespace's entries first, then the named handles."
  @spec table() :: [{String.t(), String.t() | nil, reference()}]
  def table, do: :redoubt.ns()
end
