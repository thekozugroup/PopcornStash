package com.thekozugroup.popcornstash.feature.signin

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.painterResource
import com.thekozugroup.popcornstash.R
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.thekozugroup.popcornstash.data.auth.PairingState
import com.thekozugroup.popcornstash.data.auth.PasskeyAuthenticator
import com.thekozugroup.popcornstash.feature.downloads.breakableUrl
import com.thekozugroup.popcornstash.navigation.pushTransition
import com.thekozugroup.popcornstash.ui.components.PrimaryButton
import com.thekozugroup.popcornstash.ui.components.SecondaryButton
import com.thekozugroup.popcornstash.ui.components.TopBarAction
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.icons.PopcornIcons
import com.thekozugroup.popcornstash.ui.theme.Popcorn

/**
 * Sign-in, step by step (android/DESIGN.md, Sign-in). Each step pushes in from the right
 * and Back returns one step.
 */
@Composable
fun SignInFlow(
    controller: SignInController,
    onOpenUrl: (String) -> Unit,
    passkeys: () -> PasskeyAuthenticator,
) {
    val state by controller.state.collectAsStateWithLifecycle()
    BackHandler(enabled = state.step != SignInStep.Server) { controller.onBack() }
    val density = LocalDensity.current
    val layoutDirection = LocalLayoutDirection.current
    val reduceMotion = Popcorn.reduceMotion
    AnimatedContent(
        targetState = state.step,
        transitionSpec = { pushTransition(targetState.ordinal > initialState.ordinal, density, reduceMotion, layoutDirection) },
        label = "sign-in steps",
        modifier = Modifier.fillMaxSize().background(Popcorn.colors.page),
    ) { step ->
        when (step) {
            SignInStep.Server -> ServerAddressPage(state, controller::onAddressChange, controller::onContinue)
            SignInStep.Method -> SignInMethodPage(
                state = state,
                onPasskey = { controller.onPasskey(passkeys()) },
                onPair = controller::onPair,
                onBack = { controller.onBack() },
                onOpenServer = { state.server?.let { onOpenUrl(it.url.toString()) } },
                onCheckAgain = controller::onCheckAgain,
            )
            SignInStep.Pairing -> PairingPage(
                state = state,
                onOpenApproval = { (state.pairing as? PairingState.Waiting)?.let { onOpenUrl(it.verifyUrl) } },
                onStartAgain = controller::onPair,
                onBack = { controller.onBack() },
            )
        }
    }
}

/** Step 1: the server's address. */
@Composable
fun ServerAddressPage(state: SignInUiState, onAddressChange: (String) -> Unit, onContinue: () -> Unit) {
    SignInPage(onBack = null, tag = "sign-in server") {
        Brand()
        Spacer(Modifier.height(Popcorn.spacing.xxl))
        if (state.notice != null) {
            Text(
                state.notice,
                style = Popcorn.type.body,
                color = Popcorn.colors.text,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            )
            Spacer(Modifier.height(Popcorn.spacing.l))
        }
        Heading("Sign in")
        Spacer(Modifier.height(Popcorn.spacing.xs))
        Sentence("The same address you use for PopcornStash in a browser.")
        Spacer(Modifier.height(Popcorn.spacing.l))
        OutlinedTextField(
            value = state.address,
            onValueChange = onAddressChange,
            label = { Text("Server address") },
            placeholder = { Text("stash.example.com") },
            singleLine = true,
            isError = state.addressError != null,
            supportingText = state.addressError?.let { message -> { Text(message) } },
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Go,
                autoCorrectEnabled = false,
            ),
            keyboardActions = KeyboardActions(onGo = { onContinue() }),
            shape = Popcorn.shapes.control,
            colors = fieldColors(),
            modifier = Modifier.fillMaxWidth().testTag("server address"),
        )
        Spacer(Modifier.height(Popcorn.spacing.l))
        PrimaryButton("Continue", onClick = onContinue, busy = state.checking, modifier = Modifier.fillMaxWidth())
    }
}

/** Step 2: how to sign in on this server. */
@Composable
fun SignInMethodPage(
    state: SignInUiState,
    onPasskey: () -> Unit,
    onPair: () -> Unit,
    onBack: () -> Unit,
    onOpenServer: () -> Unit,
    onCheckAgain: () -> Unit,
) {
    val server = state.server ?: return
    SignInPage(onBack = onBack, tag = "sign-in method") {
        ServerLine(server)
        Spacer(Modifier.height(Popcorn.spacing.xl))
        when {
            server.setupRequired -> {
                Heading("Finish setting up first")
                Spacer(Modifier.height(Popcorn.spacing.xs))
                Sentence(
                    "This server has no owner yet. Open it in a browser, enter the setup code from the server's log " +
                        "and make the owner's passkey. Then come back here.",
                )
                Spacer(Modifier.height(Popcorn.spacing.l))
                PrimaryButton("Open in browser", onClick = onOpenServer, glyph = PopcornIcons.OpenOnWeb, modifier = Modifier.fillMaxWidth())
                Spacer(Modifier.height(Popcorn.spacing.s))
                SecondaryButton("Check again", onClick = onCheckAgain, busy = state.checking, modifier = Modifier.fillMaxWidth())
            }
            server.passkeysOffered -> {
                Heading("How to sign in")
                Spacer(Modifier.height(Popcorn.spacing.xs))
                Sentence("Use the passkey you sign in with on the web, or pair this phone with a code.")
                Spacer(Modifier.height(Popcorn.spacing.l))
                PrimaryButton(
                    "Sign in with a passkey",
                    onClick = onPasskey,
                    busy = state.passkeyBusy,
                    glyph = PopcornIcons.Passkey,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(Popcorn.spacing.s))
                SecondaryButton("Pair with a code", onClick = onPair, enabled = !state.passkeyBusy, modifier = Modifier.fillMaxWidth())
            }
            else -> {
                Heading("How to sign in")
                Spacer(Modifier.height(Popcorn.spacing.xs))
                Sentence("Passkeys work in the app once the server lists it. Pair with a code instead.")
                Spacer(Modifier.height(Popcorn.spacing.l))
                PrimaryButton("Pair with a code", onClick = onPair, modifier = Modifier.fillMaxWidth())
            }
        }
        if (state.methodError != null) {
            Spacer(Modifier.height(Popcorn.spacing.m))
            ErrorSentence(state.methodError)
        }
    }
}

/** Step 3: the pairing code, and waiting for someone to approve it. */
@Composable
fun PairingPage(state: SignInUiState, onOpenApproval: () -> Unit, onStartAgain: () -> Unit, onBack: () -> Unit) {
    val server = state.server ?: return
    SignInPage(onBack = onBack, tag = "sign-in pairing") {
        ServerLine(server)
        Spacer(Modifier.height(Popcorn.spacing.xl))
        Heading("Pair this phone")
        Spacer(Modifier.height(Popcorn.spacing.xs))
        when (val pairing = state.pairing) {
            null, PairingState.Requesting -> {
                Sentence("Approve the code where you are signed in to PopcornStash.")
                Spacer(Modifier.height(Popcorn.spacing.xl))
                Waiting("Getting a code")
            }
            is PairingState.Waiting -> {
                Sentence("Approve this code where you are signed in to PopcornStash: in this phone's browser or on a computer.")
                Spacer(Modifier.height(Popcorn.spacing.xl))
                PairingCode(pairing.userCode)
                Spacer(Modifier.height(Popcorn.spacing.xl))
                PrimaryButton(
                    "Open PopcornStash to approve",
                    onClick = onOpenApproval,
                    glyph = PopcornIcons.OpenOnWeb,
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(Popcorn.spacing.l))
                Waiting("Waiting for approval")
            }
            is PairingState.Approved -> {
                Spacer(Modifier.height(Popcorn.spacing.l))
                Waiting("Signing in")
            }
            PairingState.Expired -> Ended("This code expired.", "Codes last ten minutes. Get a new one.", onStartAgain)
            PairingState.Denied -> Ended("This request was turned down.", "Ask the person who turned it down, then start again.", onStartAgain)
            is PairingState.Failed -> Ended(pairing.message, null, onStartAgain, action = "Try again")
        }
    }
}

// --------------------------------------------------------------- pieces ----

@Composable
private fun SignInPage(onBack: (() -> Unit)?, tag: String, content: @Composable ColumnScope.() -> Unit) {
    Box(
        Modifier
            .fillMaxSize()
            .background(Popcorn.colors.page)
            .windowInsetsPadding(WindowInsets.safeDrawing.union(WindowInsets.ime))
            .testTag(tag),
        contentAlignment = Alignment.TopCenter,
    ) {
        Column(
            Modifier
                .widthIn(max = 480.dp)
                .fillMaxWidth()
                .verticalScroll(rememberScrollState()),
        ) {
            Box(Modifier.height(64.dp).padding(start = Popcorn.spacing.xxs), contentAlignment = Alignment.CenterStart) {
                if (onBack != null) TopBarAction(PopcornIcons.Back, "Back", onBack)
            }
            Column(
                Modifier.padding(horizontal = Popcorn.spacing.gutter).padding(top = Popcorn.spacing.m, bottom = Popcorn.spacing.xl),
                content = content,
            )
        }
    }
}

@Composable
private fun Brand() {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.semantics(mergeDescendants = true) {}) {
        Image(painterResource(R.drawable.ic_launcher_flat), contentDescription = null, modifier = Modifier.size(32.dp))
        Spacer(Modifier.width(Popcorn.spacing.s))
        Text("PopcornStash", style = Popcorn.type.title, color = Popcorn.colors.text)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ServerLine(server: ServerInfo) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.semantics(mergeDescendants = true) {}) {
        LucideIcon(PopcornIcons.Server, contentDescription = null, tint = Popcorn.colors.textMuted, size = 20.dp)
        Spacer(Modifier.width(Popcorn.spacing.s))
        // The version follows the address on its line while there is room, else goes under it; it
        // is never squeezed. A long address wraps after a dot or a hyphen, never inside a word.
        FlowRow(
            Modifier.weight(1f),
            horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.xs),
            itemVerticalAlignment = Alignment.CenterVertically,
        ) {
            Text(breakableUrl(server.display), style = Popcorn.type.item, color = Popcorn.colors.text)
            Text("Version ${server.version}", style = Popcorn.type.metadata, color = Popcorn.colors.textMuted, maxLines = 1)
        }
    }
}

@Composable
private fun Heading(text: String) {
    Text(text, style = Popcorn.type.headline, color = Popcorn.colors.text, modifier = Modifier.semantics { heading() })
}

@Composable
private fun Sentence(text: String) {
    Text(text, style = Popcorn.type.body, color = Popcorn.colors.textMuted)
}

@Composable
private fun ErrorSentence(text: String) {
    Text(
        text,
        style = Popcorn.type.body,
        color = Popcorn.colors.error,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
    )
}

@Composable
private fun Waiting(text: String) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
    ) {
        CircularProgressIndicator(color = Popcorn.colors.textMuted, strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(Popcorn.spacing.s))
        Text(text, style = Popcorn.type.metadata, color = Popcorn.colors.textMuted)
    }
}

/** The code in the monospace face, its groups apart, read out letter by letter. */
@Composable
private fun PairingCode(code: String) {
    val groups = code.split('-').filter { it.isNotEmpty() }
    val spoken = groups.joinToString(", ") { group -> group.toList().joinToString(" ") }
    val style = TextStyle(
        fontFamily = FontFamily.Monospace,
        fontWeight = FontWeight.Medium,
        fontSize = 40.sp,
        lineHeight = 48.sp,
        letterSpacing = 0.06.em,
    )
    Row(
        horizontalArrangement = Arrangement.spacedBy(Popcorn.spacing.m),
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .testTag("pairing code")
            .clearAndSetSemantics { contentDescription = "Code $spoken" },
    ) {
        groups.forEach { Text(it, style = style, color = Popcorn.colors.text) }
    }
}

@Composable
private fun Ended(title: String, body: String?, onStartAgain: () -> Unit, action: String = "Start again") {
    Spacer(Modifier.height(Popcorn.spacing.l))
    Text(
        title,
        style = Popcorn.type.section,
        color = Popcorn.colors.text,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
    )
    if (body != null) {
        Spacer(Modifier.height(Popcorn.spacing.xxs))
        Sentence(body)
    }
    Spacer(Modifier.height(Popcorn.spacing.l))
    PrimaryButton(action, onClick = onStartAgain, glyph = PopcornIcons.StartAgain, modifier = Modifier.fillMaxWidth())
}

@Composable
private fun fieldColors() = OutlinedTextFieldDefaults.colors(
    focusedBorderColor = Popcorn.colors.accent,
    unfocusedBorderColor = Popcorn.colors.fieldOutline,
    focusedLabelColor = Popcorn.colors.text,
    unfocusedLabelColor = Popcorn.colors.textMuted,
    cursorColor = Popcorn.colors.text,
    focusedTextColor = Popcorn.colors.text,
    unfocusedTextColor = Popcorn.colors.text,
    focusedPlaceholderColor = Popcorn.colors.textMuted,
    unfocusedPlaceholderColor = Popcorn.colors.textMuted,
    errorBorderColor = Popcorn.colors.error,
    errorLabelColor = Popcorn.colors.error,
    errorSupportingTextColor = Popcorn.colors.error,
    errorCursorColor = Popcorn.colors.error,
)
