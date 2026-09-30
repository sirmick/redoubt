defmodule Redoubt.Shell.Stat do
  @moduledoc """
  What `stat/1` tells of a file: its path as given, its type, its size in bytes and when it last
  changed. No mode and no owner: on Redoubt a file has neither, and access is by capability.
  """

  defstruct [:path, :type, :size, :mtime]

  @type t :: %__MODULE__{
          path: String.t(),
          type: :regular | :directory | :symlink | :other,
          size: non_neg_integer(),
          mtime: DateTime.t()
        }
end
