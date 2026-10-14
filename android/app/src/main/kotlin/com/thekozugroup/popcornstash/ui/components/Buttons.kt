package com.thekozugroup.popcornstash.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LocalRippleConfiguration
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import com.thekozugroup.popcornstash.ui.icons.LucideGlyph
import com.thekozugroup.popcornstash.ui.icons.LucideIcon
import com.thekozugroup.popcornstash.ui.theme.OnAccentRipple
import com.thekozugroup.popcornstash.ui.theme.Popcorn

private val ButtonPadding = PaddingValues(horizontal = 20.dp, vertical = 12.dp)

/**
 * The one primary action of a view: Action Blue with white words, 48dp tall, 8dp corners.
 * A press lightens it with an 8% white ripple ([OnAccentRipple]), so the words keep 4.5:1.
 * While [busy] it shows a small indicator beside its label and does not take another tap.
 */
@Composable
fun PrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    busy: Boolean = false,
    glyph: LucideGlyph? = null,
) {
    val colors = Popcorn.colors
    CompositionLocalProvider(LocalRippleConfiguration provides OnAccentRipple) {
        Button(
            onClick = { if (!busy) onClick() },
            enabled = enabled,
            shape = Popcorn.shapes.control,
            colors = ButtonDefaults.buttonColors(
                containerColor = colors.accent,
                contentColor = colors.onAccent,
                disabledContainerColor = colors.subtle,
                disabledContentColor = colors.textMuted,
            ),
            contentPadding = ButtonPadding,
            modifier = modifier
                .defaultMinSize(minHeight = 48.dp)
                .semantics { if (busy) stateDescription = "Working" },
        ) {
            ButtonContent(text, glyph, busy, colors.onAccent)
        }
    }
}

/** A second choice beside the primary one: outlined in the divider colour, text in the text colour. */
@Composable
fun SecondaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    busy: Boolean = false,
    glyph: LucideGlyph? = null,
) {
    val colors = Popcorn.colors
    OutlinedButton(
        onClick = { if (!busy) onClick() },
        enabled = enabled,
        shape = Popcorn.shapes.control,
        border = BorderStroke(1.dp, colors.outline),
        colors = ButtonDefaults.outlinedButtonColors(contentColor = colors.text, disabledContentColor = colors.textMuted),
        contentPadding = ButtonPadding,
        modifier = modifier.defaultMinSize(minHeight = 48.dp),
    ) {
        ButtonContent(text, glyph, busy, colors.text)
    }
}

/** A quiet action in running text or a section header ("Start again", "Accept all"). */
@Composable
fun QuietButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true) {
    TextButton(
        onClick = onClick,
        enabled = enabled,
        shape = Popcorn.shapes.control,
        colors = ButtonDefaults.textButtonColors(contentColor = Popcorn.colors.text),
        modifier = modifier.defaultMinSize(minHeight = 48.dp),
    ) {
        Text(text, style = Popcorn.type.action)
    }
}

@Composable
private fun ButtonContent(text: String, glyph: LucideGlyph?, busy: Boolean, tint: Color) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        when {
            busy -> {
                CircularProgressIndicator(color = tint, strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(10.dp))
            }
            glyph != null -> {
                LucideIcon(glyph, contentDescription = null, tint = tint, size = 18.dp)
                Spacer(Modifier.width(8.dp))
            }
        }
        Text(text, style = Popcorn.type.action)
    }
}
