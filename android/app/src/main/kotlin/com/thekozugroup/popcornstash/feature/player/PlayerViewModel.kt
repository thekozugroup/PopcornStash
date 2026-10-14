package com.thekozugroup.popcornstash.feature.player

import android.app.Application
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.thekozugroup.popcornstash.data.device.PlaybackCapabilities
import com.thekozugroup.popcornstash.data.session.Session
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Keeps the player for [session] alive across the activity being made again (picture-in-picture,
 * a resize, a theme change): the ExoPlayer instance and the [PlayerController]. When the player
 * really leaves, the controller saves the place and ends the server's stream on the session's
 * scope, which outlives this view model.
 */
class PlayerViewModel(app: Application, session: Session) : ViewModel() {
    val engine = ExoPlayerEngine(app, session.http)

    val controller = PlayerController(
        api = ApiPlayerApi(session.api),
        engine = engine,
        // Asking MediaCodecList about every decoder takes a moment: off the main thread.
        deviceCapabilities = { withContext(Dispatchers.Default) { PlaybackCapabilities.request(app) } },
        scope = viewModelScope,
        outlive = session.scope,
    )

    override fun onCleared() {
        controller.close()
    }

    companion object {
        fun factory(app: Application, session: Session) = viewModelFactory {
            initializer { PlayerViewModel(app, session) }
        }
    }
}
