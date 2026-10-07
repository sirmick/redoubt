defmodule Redoubt.Keys do
  @moduledoc """
  `keyd` (docs/servers/keyd.md), over its generated client: the public key, and whether `keyd`
  holds a key. Signing is the servers' that `keyd` grants it to, never a session's own: a session
  asks what its handle lets it ask, and `keyd` checks.
  """

  alias Redoubt.Wire.Client.Keyd

  @doc "The connection to `keyd`, the named handle `keyd`, if the session was given one."
  @spec connection() :: {:ok, reference()} | {:error, atom()}
  def connection, do: Redoubt.Namespace.handle("keyd")

  @doc "The box's public key."
  @spec public_key() :: {:ok, binary()} | {:error, atom()}
  def public_key do
    with {:ok, keyd} <- connection(), {:ok, %{key: key}} <- Keyd.public_key(keyd), do: {:ok, key}
  end

  @doc "Whether `keyd` holds the key `key`."
  @spec holds?(binary()) :: {:ok, boolean()} | {:error, atom()}
  def holds?(key) when is_binary(key) do
    with {:ok, keyd} <- connection(), {:ok, %{held: held}} <- Keyd.holds(keyd, key), do: {:ok, held != 0}
  end
end
