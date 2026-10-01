defmodule Redoubt.Commandlet.UsageError do
  @moduledoc """
  A command called with an argument that does not fit its parameter: the message names the
  parameter, what it takes and what it got, and shows the command's usage.
  """

  defexception [:message, :command]
end
