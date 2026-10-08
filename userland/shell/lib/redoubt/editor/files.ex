defmodule Redoubt.Editor.Files do
  @moduledoc """
  What the editor and the file manager do to files (docs/userland/shell.md, "The editor's
  files"): read one for editing, save it, list a directory, and copy, move, make and remove what
  a listing shows. Everything is done with the session's own authority, through `File`, and
  nothing here reads a path, a name or an action out of a file's contents.

  - **A file is read whole, up to `max_bytes/0`.** A larger one, or one that is not a regular
    file, is refused by name. What is read comes back as bytes, with whether it is UTF-8 and a
    digest, for the save to check against.
  - **A save reaches only its own path.** It writes a new file beside it, in the same directory,
    under a name of the module's own making, and renames that over the path; a write, close or
    rename that fails removes the new file. A file changed on disk since it was read is not overwritten unless the
    caller says so.
  - **The panes act only on what they list.** A name that holds `/` or NUL, or is empty, `.` or
    `..`, is shown and refused: no operation joins it to a directory. Every other name is acted
    on as its listed directory joined with it, and shown with any control or bidirectional
    character drawn visibly.
  """

  alias Redoubt.Term.Text

  # The largest file the editor opens: it is held whole, as lines, in the screen's heap.
  @max_bytes 2 * 1024 * 1024

  @typedoc "A listed entry: its name, the name as shown, whether it may be acted on, and its kind."
  @type entry :: %{
          name: String.t(),
          shown: String.t(),
          ok: boolean(),
          type: :directory | :regular | :other | :refused,
          size: non_neg_integer()
        }

  @typedoc "What a save checks the file on disk against: what was read, that there was none, or nothing."
  @type expected :: binary() | :absent | :any

  @doc "The largest file, in bytes, that `read/1` gives."
  @spec max_bytes() :: pos_integer()
  def max_bytes, do: @max_bytes

  @doc """
  The file at `path`, whole: `%{bytes: bytes, utf8: boolean, digest: digest}`. Refused:
  `{:error, {:too_large, size, max_bytes}}`, `{:error, {:not_a_file, type}}`, or `File`'s reason.
  """
  @spec read(Path.t()) :: {:ok, map()} | {:error, term()}
  def read(path) do
    with {:ok, %File.Stat{type: :regular, size: size}} <- File.stat(path),
         :ok <- fits(size),
         {:ok, bytes} <- read_capped(path),
         :ok <- fits(byte_size(bytes)) do
      {:ok, %{bytes: bytes, utf8: String.valid?(bytes), digest: digest(bytes)}}
    else
      {:ok, %File.Stat{type: type}} -> {:error, {:not_a_file, type}}
      {:error, _reason} = error -> error
    end
  end

  defp fits(size) when size > @max_bytes, do: {:error, {:too_large, size, @max_bytes}}
  defp fits(_size), do: :ok

  # The file's bytes, up to one past the cap: a file grown past it since its stat is refused
  # without being read whole.
  defp read_capped(path) do
    case File.open(path, [:read, :binary], &read_upto(&1, @max_bytes + 1, [])) do
      {:ok, result} -> result
      {:error, _reason} = error -> error
    end
  end

  defp read_upto(_device, 0, acc), do: {:ok, IO.iodata_to_binary(acc)}

  defp read_upto(device, left, acc) do
    case :file.read(device, left) do
      {:ok, chunk} -> read_upto(device, left - byte_size(chunk), [acc | chunk])
      :eof -> {:ok, IO.iodata_to_binary(acc)}
      {:error, _reason} = error -> error
    end
  end

  @doc """
  Saves `data` as the file at `path`, through a new file beside it and a rename over it, and
  returns the digest of what was saved. `expected` is the digest `read/1` gave, `:absent` for a
  file that was not there, or `:any` to overwrite whatever is: a file that is not as expected is
  `{:error, :changed}`, and left as it is.
  """
  @spec save(Path.t(), iodata(), expected()) :: {:ok, binary()} | {:error, term()}
  def save(path, data, expected) do
    bytes = IO.iodata_to_binary(data)

    with :ok <- unchanged(path, expected) do
      temp = Path.join(Path.dirname(path), "." <> Path.basename(path) <> ".saving-" <> random())

      # A temp that could not be made is not ours to remove: on :eexist it is another saver's.
      with {:ok, device} <- File.open(temp, [:write, :exclusive, :binary]) do
        written = :file.write(device, bytes)
        closed = File.close(device)

        with :ok <- written, :ok <- closed, :ok <- File.rename(temp, path) do
          {:ok, digest(bytes)}
        else
          {:error, _reason} = error ->
            _ = File.rm(temp)
            error
        end
      end
    end
  end

  defp unchanged(_path, :any), do: :ok

  defp unchanged(path, :absent) do
    case File.lstat(path) do
      {:error, :enoent} -> :ok
      {:ok, _stat} -> {:error, :changed}
      {:error, _reason} = error -> error
    end
  end

  defp unchanged(path, digest) when is_binary(digest) do
    case read_capped(path) do
      {:ok, bytes} -> if digest(bytes) == digest, do: :ok, else: {:error, :changed}
      {:error, :enoent} -> {:error, :changed}
      {:error, _reason} = error -> error
    end
  end

  defp digest(bytes), do: :crypto.hash(:sha256, bytes)
  defp random, do: Base.encode16(:crypto.strong_rand_bytes(6), case: :lower)

  @doc "Whether a listed name may be acted on: not empty, `.` or `..`, and holding no `/` or NUL."
  @spec name_ok?(String.t()) :: boolean()
  def name_ok?(name) when is_binary(name),
    do: name not in ["", ".", ".."] and not String.contains?(name, ["/", <<0>>])

  @doc """
  The directory's entries, by name, each with its name as shown and whether it may be acted on.
  A name that may not is neither looked at nor joined to the directory: its type is `:refused`.
  """
  @spec list(Path.t()) :: {:ok, [entry()]} | {:error, term()}
  def list(dir) do
    with {:ok, names} <- File.ls(dir) do
      {:ok, names |> Enum.sort() |> Enum.map(&entry(dir, &1))}
    end
  end

  defp entry(dir, name) do
    shown = Text.visible(name)

    if name_ok?(name) do
      {type, size} =
        case File.lstat(Path.join(dir, name)) do
          {:ok, %File.Stat{type: type, size: size}} when type in [:directory, :regular] -> {type, size}
          {:ok, %File.Stat{size: size}} -> {:other, size}
          {:error, _reason} -> {:other, 0}
        end

      %{name: name, shown: shown, ok: true, type: type, size: size}
    else
      %{name: name, shown: shown, ok: false, type: :refused, size: 0}
    end
  end

  @doc "Copies `name` in `dir` to the same name in `to_dir`: a file, or a directory and all it holds."
  @spec copy(Path.t(), String.t(), Path.t()) :: :ok | {:error, term()}
  def copy(dir, name, to_dir) do
    with {:ok, src, dst} <- endpoints(dir, name, to_dir) do
      if File.dir?(src) do
        case File.cp_r(src, dst) do
          {:ok, _copied} -> :ok
          {:error, reason, _file} -> {:error, reason}
        end
      else
        File.cp(src, dst)
      end
    end
  end

  @doc """
  Moves `name` in `dir` to the same name in `to_dir`: a rename within a volume, a copy and a
  removal across volumes, as `mv` does.
  """
  @spec move(Path.t(), String.t(), Path.t()) :: :ok | {:error, term()}
  def move(dir, name, to_dir) do
    with {:ok, src, dst} <- endpoints(dir, name, to_dir) do
      try do
        Redoubt.Shell.Helpers.mv(src, dst)
      rescue
        e in [File.Error, File.CopyError, File.RenameError] -> {:error, e.reason}
      end
    end
  end

  @doc "Makes the directory `name` in `dir`."
  @spec mkdir(Path.t(), String.t()) :: :ok | {:error, term()}
  def mkdir(dir, name) do
    if name_ok?(name), do: File.mkdir(Path.join(dir, name)), else: {:error, :bad_name}
  end

  @doc "Removes `name` in `dir`: a file, or a directory and all it holds."
  @spec remove(Path.t(), String.t()) :: :ok | {:error, term()}
  def remove(dir, name) do
    if name_ok?(name) do
      path = Path.join(dir, name)

      case File.lstat(path) do
        {:ok, %File.Stat{type: :directory}} ->
          case File.rm_rf(path) do
            {:ok, _removed} -> :ok
            {:error, reason, _file} -> {:error, reason}
          end

        {:ok, _file} ->
          File.rm(path)

        {:error, _reason} = error ->
          error
      end
    else
      {:error, :bad_name}
    end
  end

  # What a copy or a move acts on: the name in its listed directory, to the same name in the
  # other. Nothing is overwritten, and a directory is never put inside itself.
  defp endpoints(dir, name, to_dir) do
    src = Path.join(dir, name)
    dst = Path.join(to_dir, name)

    cond do
      not name_ok?(name) -> {:error, :bad_name}
      match?({:ok, _}, File.lstat(dst)) -> {:error, :eexist}
      inside?(Path.expand(to_dir), Path.expand(src)) -> {:error, :einval}
      true -> {:ok, src, dst}
    end
  end

  defp inside?(path, dir), do: path == dir or String.starts_with?(path, dir <> "/")
end
