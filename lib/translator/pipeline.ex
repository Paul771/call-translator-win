defmodule Translator.Pipeline do
  @moduledoc """
  GenServer tracking state for a single audio processing pipeline direction.

  Accumulates transcripts and translations received from the AudioEngine.
  """

  use GenServer
  require Logger

  defstruct [:direction, :status, transcripts: [], translations: []]

  # --- Client API ---

  def start_link(direction) when direction in [:outgoing, :incoming] do
    GenServer.start_link(__MODULE__, direction)
  end

  @doc "Pushes an event from AudioEngine into this pipeline."
  @spec handle_event(pid(), map()) :: :ok
  def handle_event(pid, event) do
    GenServer.cast(pid, {:event, event})
  end

  @doc "Returns the direction of this pipeline."
  @spec get_direction(pid()) :: atom()
  def get_direction(pid) do
    GenServer.call(pid, :get_direction)
  end

  @doc "Returns accumulated transcripts and translations."
  @spec get_history(pid()) :: map()
  def get_history(pid) do
    GenServer.call(pid, :get_history)
  end

  # --- Server Callbacks ---

  @impl true
  def init(direction) do
    Logger.info("Pipeline started for direction: #{direction}")
    {:ok, %__MODULE__{direction: direction, status: :active}}
  end

  @impl true
  def handle_cast({:event, %{"event" => "transcript", "text" => text} = event}, state) do
    timestamp = Map.get(event, "timestamp", System.system_time(:millisecond))

    entry = %{text: text, timestamp: timestamp}

    Logger.debug("Pipeline #{state.direction} transcript: #{text}",
      direction: state.direction,
      pipeline: "transcript"
    )

    {:noreply, %{state | transcripts: [entry | state.transcripts]}}
  end

  def handle_cast({:event, %{"event" => "translation", "text" => text} = event}, state) do
    timestamp = Map.get(event, "timestamp", System.system_time(:millisecond))

    entry = %{text: text, timestamp: timestamp}

    Logger.debug("Pipeline #{state.direction} translation: #{text}",
      direction: state.direction,
      pipeline: "translation"
    )

    {:noreply, %{state | translations: [entry | state.translations]}}
  end

  def handle_cast({:event, event}, state) do
    Logger.debug("Pipeline #{state.direction} unhandled event: #{inspect(event)}")
    {:noreply, state}
  end

  @impl true
  def handle_call(:get_direction, _from, state) do
    {:reply, state.direction, state}
  end

  @impl true
  def handle_call(:get_history, _from, state) do
    history = %{
      direction: state.direction,
      status: state.status,
      transcripts: Enum.reverse(state.transcripts),
      translations: Enum.reverse(state.translations)
    }

    {:reply, history, state}
  end
end
