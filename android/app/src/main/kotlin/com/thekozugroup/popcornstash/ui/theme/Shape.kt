package com.thekozugroup.popcornstash.ui.theme

import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Shapes
import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.dp

/**
 * One radius scale (android/DESIGN.md, Shape): tags 6, controls 8, media 10,
 * toolbar squares 12, panels, sheets and the toolbar 16, the avatar a circle.
 */
@Immutable
data class PopcornShapes(
    val tag: Shape = RoundedCornerShape(6.dp),
    val control: Shape = RoundedCornerShape(8.dp),
    val media: Shape = RoundedCornerShape(10.dp),
    val panel: Shape = RoundedCornerShape(16.dp),
    val sheet: Shape = RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp),
    val avatar: Shape = CircleShape,
)

val DefaultPopcornShapes = PopcornShapes()

fun popcornMaterialShapes(): Shapes = Shapes(
    extraSmall = RoundedCornerShape(6.dp),
    small = RoundedCornerShape(8.dp),
    medium = RoundedCornerShape(10.dp),
    large = RoundedCornerShape(16.dp),
    extraLarge = RoundedCornerShape(16.dp),
)
