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

  @summary "Clear the screen"
  @help """
  Clears the terminal and puts the cursor at its top left, where the next prompt is drawn. Output
  that is not the shell's terminal, a captured one or a file, is left as it is.
  """
  @examples [{"clear()", "a clean screen"}]
  defcommand clear() do
    case Redoubt.Shell.Driver.of_group() do
      nil ->
        :ok

      driver ->
        ref = make_ref()
        send(driver, {:redoubt_clear, self(), ref})

        receive do
          {:redoubt_cleared, ^ref} -> :ok
        end
    end
  end
end
