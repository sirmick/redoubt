defmodule Redoubt.File do
  @moduledoc """
  File operations Redoubt has and `File` does not (docs/userland/files.md, "Copying, moving,
  removing and binds"), over beamlet's natives. A thin layer: it adds no authority, and every
  refusal is the platform's, as the POSIX error `File` would give.
  """

  # beamlet's natives (docs/userland/beamlet.md, "Natives"): no module, so nothing to check at compile time.
  @compile {:no_warn_undefined, :redoubt}

  @doc """
  Copies the file `from` to the new file `to`, the file server copying it itself, so no byte
  crosses into the VM: `{:ok, bytes}`. `{:error, :exdev}` when the two are on different servers,
  and `{:error, :enotsup}` where the servers do not copy (the host, or a VM without beamlet's
  natives); the caller then copies through the VM.
  """
  @spec copy_file(Path.t(), Path.t()) :: {:ok, non_neg_integer()} | {:error, atom()}
  def copy_file(from, to) do
    :redoubt.copy_file(IO.chardata_to_string(from), IO.chardata_to_string(to))
  rescue
    UndefinedFunctionError -> {:error, :enotsup}
  end
end
