defmodule Redoubt.Shell.Control do
  @moduledoc "The commands that control the shell itself, imported at the prompt."

  use Redoubt.Commandlet, area: "Shell"

  @summary "End the shell"
  @help """
  Ends the shell, as Ctrl+D does. Typed alone on a line, exit needs no parentheses.
  """
  @examples [{"exit()", "end the shell"}]
  defcommand exit() do
    # The evaluator catches this and tells the shell to end: the one way a line ends the loop.
    throw(:redoubt_shell_exit)
  end
end
