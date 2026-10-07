defmodule Redoubt.Commandlet.Registry do
  @moduledoc """
  Every commandlet of the shell, found by looking: a module of the shell's application that uses
  `Redoubt.Commandlet` joins it by being in the application, with nothing to list anywhere. Its
  commands are imported at the prompt (`imports/0`) and listed by `help`.
  """

  alias Redoubt.Commandlet

  @app :redoubt_shell

  @doc "Every commandlet, by name."
  @spec all() :: [Commandlet.t()]
  def all, do: modules() |> Enum.flat_map(& &1.__commandlets__()) |> Enum.sort_by(&Atom.to_string(&1.name))

  @doc "The modules that declare commandlets, in order."
  @spec modules() :: [module()]
  def modules do
    # Loading is idempotent, and makes the application's module list known where nothing has
    # started it, as under a test runner.
    _ = Application.load(@app)

    # The wire's codecs and clients, compiled into the application, declare none: they are not
    # loaded to be asked, so the prompt does not hold them until a session calls a server.
    for module <- Enum.sort(Application.spec(@app, :modules) || []),
        not String.starts_with?(Atom.to_string(module), "Elixir.Redoubt.Wire."),
        Code.ensure_loaded?(module),
        function_exported?(module, :__commandlets__, 0),
        do: module
  end

  @doc """
  The commandlet named `name`, an atom or a string. A string is compared as a string, so a name
  typed at the prompt never becomes an atom that did not exist.
  """
  @spec fetch(atom() | String.t()) :: {:ok, Commandlet.t()} | :error
  def fetch(name) when is_atom(name) or is_binary(name) do
    wanted = to_string(name)

    case Enum.find(all(), &(Atom.to_string(&1.name) == wanted)) do
      nil -> :error
      commandlet -> {:ok, commandlet}
    end
  end

  @doc "An `import` of each module's commands, and nothing else of it, as quoted code for the prompt."
  @spec imports() :: Macro.t()
  def imports do
    imports =
      for module <- modules() do
        only = Enum.flat_map(module.__commandlets__(), &Commandlet.arities/1)
        quote do: import(unquote(module), only: unquote(only))
      end

    {:__block__, [], imports}
  end
end
