package com.ezoo.download

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.LayoutDirection

/** The look of the first sketches (Cairo, violet to blue). */
object Ui {
    val Violet = Color(0xFF6D4AFF)
    val Blue = Color(0xFF3B82F6)
    val Ink = Color(0xFF151826)
    val Muted = Color(0xFF7B80A3)
    val Label = Color(0xFF555B80)
    val Page = Color(0xFFF6F7FB)
    val Line = Color(0xFFE6E8F2)
    val Soft = Color(0xFFF0F1F8)
    val Chip = Color(0xFFECEEFA)
    val Track = Color(0xFFE7E9F5)
    val Field = Color(0xFFFAFBFF)
    val FieldLine = Color(0xFFDFE2F0)
    val Red = Color(0xFFD03A3A)
    val Green = Color(0xFF1F9D55)
    val Gradient = Brush.linearGradient(listOf(Violet, Blue))
    val Bar = Brush.horizontalGradient(listOf(Violet, Blue))

    val Cairo = FontFamily(
        Font(R.font.cairo_400, FontWeight.Normal),
        Font(R.font.cairo_700, FontWeight.Bold),
        Font(R.font.cairo_800, FontWeight.ExtraBold),
    )
}

@Composable
fun EzooTheme(content: @Composable () -> Unit) {
    val base = Typography()
    fun TextStyle.cairo() = copy(fontFamily = Ui.Cairo)
    val type = Typography(
        displayLarge = base.displayLarge.cairo(), displayMedium = base.displayMedium.cairo(),
        displaySmall = base.displaySmall.cairo(), headlineLarge = base.headlineLarge.cairo(),
        headlineMedium = base.headlineMedium.cairo(), headlineSmall = base.headlineSmall.cairo(),
        titleLarge = base.titleLarge.cairo(), titleMedium = base.titleMedium.cairo(),
        titleSmall = base.titleSmall.cairo(), bodyLarge = base.bodyLarge.cairo(),
        bodyMedium = base.bodyMedium.cairo(), bodySmall = base.bodySmall.cairo(),
        labelLarge = base.labelLarge.cairo(), labelMedium = base.labelMedium.cairo(),
        labelSmall = base.labelSmall.cairo(),
    )
    MaterialTheme(
        colorScheme = lightColorScheme(
            primary = Ui.Violet, background = Ui.Page, surface = Color.White,
            onSurface = Ui.Ink, surfaceContainerHigh = Color.White, surfaceContainer = Color.White,
        ),
        typography = type,
    ) {
        // the app is in Arabic: right to left on every phone
        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Rtl, content = content)
    }
}
